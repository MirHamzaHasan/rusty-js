//! V8 execution and a Tokio-driven host Promise bridge, owned by one thread.

mod async_ops;
mod modules;
mod timers;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::io::{self, Write};
use std::rc::Rc;
use std::sync::OnceLock;

use async_ops::{HostState, SharedState};

static V8_INITIALIZED: OnceLock<()> = OnceLock::new();

/// A host-owned diagnostic: no V8 handles escape the execution boundary.
#[derive(Debug, Clone)]
pub struct JsError {
    pub message: String,
    pub origin: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub stack: Option<String>,
}

impl JsError {
    fn host(message: &str, origin: &str) -> Self {
        Self {
            message: message.into(),
            origin: origin.into(),
            line: None,
            column: None,
            stack: None,
        }
    }
}

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.origin)?;
        if let Some(line) = self.line {
            write!(f, ":{line}")?;
            if let Some(column) = self.column {
                write!(f, ":{column}")?;
            }
        }
        write!(f, ": {}", self.message)?;
        if let Some(stack) = &self.stack {
            write!(f, "\n{stack}")?;
        }
        Ok(())
    }
}

impl std::error::Error for JsError {}

/// One context and isolate, accessed and dropped on their creating thread.
/// Multiple instances on one thread must be dropped in reverse creation order,
/// as required by V8's entered-isolate stack.
pub struct JsRuntime {
    // Fields drop in declaration order: persistent handles must go first.
    context: v8::Global<v8::Context>,
    state: SharedState,
    modules: modules::SharedModules,
    isolate: v8::OwnedIsolate,
}

impl JsRuntime {
    pub fn new() -> Result<Self, JsError> {
        V8_INITIALIZED.get_or_init(|| {
            let platform = v8::new_default_platform(0, false).make_shared();
            v8::V8::initialize_platform(platform);
            v8::V8::initialize();
        });

        let mut isolate = v8::Isolate::new(Default::default());
        isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
        isolate.set_promise_reject_callback(async_ops::promise_rejected);
        let state = Rc::new(RefCell::new(HostState::default()));
        isolate.set_slot(state.clone());
        let modules = Rc::new(RefCell::new(modules::ModuleMap::default()));
        isolate.set_slot(modules.clone());
        let context = {
            v8::scope!(let scope, &mut isolate);
            let context = v8::Context::new(scope, Default::default());
            let scope = &mut v8::ContextScope::new(scope, context);
            install_console(scope, context)?;
            async_ops::install(scope, context)
                .ok_or_else(|| JsError::host("could not install Rusty.sleep", "<host>"))?;
            timers::install(scope, context)
                .ok_or_else(|| JsError::host("could not install callback timers", "<host>"))?;
            v8::Global::new(scope, context)
        };
        Ok(Self {
            context,
            state,
            modules,
            isolate,
        })
    }

    /// Compile and execute a classic script in this runtime's persistent context.
    /// Ordinary guest exceptions become errors; the runtime can execute again.
    pub fn execute(&mut self, source: &str, origin: &str) -> Result<(), JsError> {
        self.state.borrow_mut().origin = origin.into();
        let execution = self.execute_script(source, origin);
        // Complete the turn even when top-level JS throws; preserve its primary error.
        let checkpoint = self.checkpoint();
        let result = execution.and(checkpoint);
        if result.is_err() {
            self.cancel_pending();
        }
        result
    }

    fn execute_script(&mut self, source: &str, origin: &str) -> Result<(), JsError> {
        v8::scope!(let scope, &mut self.isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        v8::tc_scope!(let scope, scope);

        let filename = v8::String::new(scope, origin)
            .ok_or_else(|| JsError::host("could not allocate script origin", origin))?;
        let source = v8::String::new(scope, source)
            .ok_or_else(|| JsError::host("could not allocate script source", origin))?;
        let script_origin = v8::ScriptOrigin::new(
            scope,
            filename.into(),
            0,
            0,
            false,
            0,
            None,
            false,
            false,
            false,
            None,
        );
        let Some(script) = v8::Script::compile(scope, source, Some(&script_origin)) else {
            return Err(caught_error(scope, origin));
        };
        if script.run(scope).is_none() {
            return Err(caught_error(scope, origin));
        }
        Ok(())
    }

    /// Drive queued host operations until idle. Call inside an active Tokio runtime
    /// with its time driver enabled. No V8 scope or Local handle crosses `.await`.
    /// Cancelling this future aborts its tasks and releases all pending resolver roots.
    pub async fn run_event_loop(&mut self) -> Result<(), JsError> {
        let _cleanup = async_ops::LoopGuard(self.state.clone());
        let _module_cleanup = modules::EvaluationGuard(self.modules.clone());
        if tokio::runtime::Handle::try_current().is_err() {
            return Err(JsError::host("run_event_loop requires Tokio", "<host>"));
        }
        let mut tasks = tokio::task::JoinSet::new();
        let mut task_operations = HashMap::new();
        let mut prefer_host_completion = false;
        loop {
            let requests = std::mem::take(&mut self.state.borrow_mut().requests);
            for request in requests {
                let handle = tasks.spawn(request.future);
                task_operations.insert(handle.id(), request.id);
            }
            // Only IDs and deadlines escape this borrow; timer roots stay here.
            let next_timer = self
                .state
                .borrow()
                .timers
                .iter()
                .map(|(id, timer)| (*id, timer.deadline))
                .min_by_key(|(id, deadline)| (*deadline, *id));
            let ready_host = if prefer_host_completion {
                tasks.try_join_next_with_id()
            } else {
                None
            };
            let completion = if let Some(completion) = ready_host {
                Some(completion)
            } else if let Some((id, deadline)) = next_timer {
                if deadline <= tokio::time::Instant::now() {
                    let callback = self.fire_timer(id);
                    let checkpoint = self.checkpoint();
                    callback.and(checkpoint)?;
                    self.rearm_interval(id);
                    prefer_host_completion = true;
                    // Give Rust tasks a chance to make progress even when JS keeps
                    // registering already-due zero-delay timers.
                    tokio::task::yield_now().await;
                    continue;
                }
                if tasks.is_empty() {
                    tokio::time::sleep_until(deadline).await;
                    continue;
                }
                // join_next_with_id is cancellation-safe: a deadline cannot lose
                // an already-completed host task when this timeout wins.
                match tokio::time::timeout_at(deadline, tasks.join_next_with_id()).await {
                    Ok(completion) => completion,
                    Err(_) => continue,
                }
            } else {
                tasks.join_next_with_id().await
            };
            // Neither a JS-only Promise nor a cleared timer keeps the loop alive.
            let Some(completion) = completion else {
                return self.check_modules(true);
            };
            prefer_host_completion = false;
            let (task_id, outcome) = match completion {
                Ok((id, outcome)) => (id, outcome),
                Err(error) => (
                    error.id(),
                    Err(format!("host operation task failed: {error}")),
                ),
            };
            let id = task_operations
                .remove(&task_id)
                .ok_or_else(|| JsError::host("unknown host task completion", "<host>"))?;
            self.settle(id, outcome)?;
            self.checkpoint()?;
        }
    }

    /// Cancel queued operations and timers before starting the event loop.
    /// Their JS Promises remain pending; resolver and callback roots are released.
    pub fn cancel_pending(&mut self) {
        self.state.borrow_mut().cancel();
        self.modules.borrow_mut().cancel();
    }

    fn fire_timer(&mut self, id: u64) -> Result<(), JsError> {
        v8::scope!(let scope, &mut self.isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        v8::tc_scope!(let scope, scope);
        let (callback, arguments, origin, repeat) = {
            let state = self.state.borrow();
            let Some(timer) = state.timers.get(&id) else {
                return Ok(());
            };
            (
                v8::Local::new(scope, &timer.callback),
                timer
                    .arguments
                    .iter()
                    .map(|value| v8::Local::new(scope, value))
                    .collect::<Vec<_>>(),
                timer.origin.clone(),
                timer.interval.is_some(),
            )
        };
        if !repeat {
            self.state.borrow_mut().timers.remove(&id);
        }
        self.state.borrow_mut().origin = origin.clone();
        // No registry borrow survives a call into JS: callbacks may clear or add timers.
        let receiver = context.global(scope);
        if callback.call(scope, receiver.into(), &arguments).is_none() {
            return Err(caught_error(scope, &origin));
        }
        Ok(())
    }

    fn rearm_interval(&mut self, id: u64) {
        if let Some(timer) = self.state.borrow_mut().timers.get_mut(&id)
            && let Some(delay) = timer.interval
        {
            // Fixed delay after callback AND checkpoint; no missed-tick bursts.
            timer.deadline = tokio::time::Instant::now() + delay;
        }
    }

    fn settle(&mut self, id: u64, outcome: Result<(), String>) -> Result<(), JsError> {
        let operation = self.state.borrow_mut().operations.remove(&id);
        let Some(operation) = operation else {
            return Ok(());
        };
        self.state.borrow_mut().origin = operation.origin.clone();
        v8::scope!(let scope, &mut self.isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        v8::tc_scope!(let scope, scope);
        let resolver = v8::Local::new(scope, &operation.resolver);
        let settled = match outcome {
            Ok(()) => {
                let value = v8::undefined(scope);
                resolver.resolve(scope, value.into())
            }
            Err(message) => {
                let message = v8::String::new(scope, &message).ok_or_else(|| {
                    JsError::host("could not allocate host error", &operation.origin)
                })?;
                let error = v8::Exception::error(scope, message);
                resolver.reject(scope, error)
            }
        };
        if settled != Some(true) {
            return Err(caught_error(scope, &operation.origin));
        }
        Ok(())
    }

    fn checkpoint(&mut self) -> Result<(), JsError> {
        self.checkpoint_promises()?;
        self.check_modules(false)
    }

    fn checkpoint_promises(&mut self) -> Result<(), JsError> {
        v8::scope!(let scope, &mut self.isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        scope.perform_microtask_checkpoint();
        // A handler attached anywhere in this checkpoint removes the candidate.
        let rejections = std::mem::take(&mut self.state.borrow_mut().rejections);
        if let Some(rejection) = rejections.into_iter().next() {
            let reason = v8::Local::new(scope, &rejection.reason);
            let message = v8::Exception::create_message(scope, reason);
            let mut error = message_error(scope, message, &rejection.origin);
            error.message = format!("Unhandled promise rejection: {}", error.message);
            return Err(error);
        }
        Ok(())
    }
}

impl Drop for JsRuntime {
    fn drop(&mut self) {
        self.state.borrow_mut().cancel();
        self.modules.borrow_mut().clear();
        self.isolate.remove_slot::<SharedState>();
        self.isolate.remove_slot::<modules::SharedModules>();
    }
}

fn install_console(
    scope: &mut v8::PinScope,
    context: v8::Local<v8::Context>,
) -> Result<(), JsError> {
    let failed = || JsError::host("could not install console.log", "<host>");
    let console = v8::Object::new(scope);
    let log = v8::Function::new(scope, console_log).ok_or_else(failed)?;
    let log_key = v8::String::new(scope, "log").ok_or_else(failed)?;
    let console_key = v8::String::new(scope, "console").ok_or_else(failed)?;
    if console.set(scope, log_key.into(), log.into()) != Some(true)
        || context
            .global(scope)
            .set(scope, console_key.into(), console.into())
            != Some(true)
    {
        return Err(failed());
    }
    Ok(())
}

fn console_log(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _result: v8::ReturnValue,
) {
    let mut line = String::new();
    for index in 0..args.length() {
        // ToString may execute guest code and throw. Leave that exception pending.
        let Some(value) = args.get(index).to_string(scope) else {
            return;
        };
        if index > 0 {
            line.push(' ');
        }
        line.push_str(&value.to_rust_string_lossy(scope));
    }
    line.push('\n');
    if let Err(error) = io::stdout().lock().write_all(line.as_bytes())
        && let Some(message) = v8::String::new(scope, &format!("stdout write failed: {error}"))
    {
        let exception = v8::Exception::error(scope, message);
        scope.throw_exception(exception);
    }
}

fn caught_error(
    scope: &mut v8::PinnedRef<'_, v8::TryCatch<v8::HandleScope>>,
    origin: &str,
) -> JsError {
    let mut error = scope
        .message()
        .map(|message| message_error(scope, message, origin))
        .unwrap_or_else(|| JsError::host("JavaScript execution failed", origin));
    error.stack = scope
        .stack_trace()
        .and_then(|value| value.to_string(scope))
        .map(|value| value.to_rust_string_lossy(scope));
    error
}

fn message_error(scope: &v8::PinScope, message: v8::Local<v8::Message>, origin: &str) -> JsError {
    // Use V8's message rather than invoking a thrown object's user-defined toString.
    let mut error = JsError::host(&message.get(scope).to_rust_string_lossy(scope), origin);
    if let Some(filename) = message
        .get_script_resource_name(scope)
        .and_then(|value| v8::Local::<v8::String>::try_from(value).ok())
    {
        let filename = filename.to_rust_string_lossy(scope);
        if !filename.is_empty() {
            error.origin = filename;
        }
    }
    error.line = message.get_line_number(scope);
    // Host-generated errors may use usize::MAX to mean an unavailable column.
    error.column = message.get_start_column().checked_add(1);
    error
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_survives_guest_errors_and_tokio_suspension() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async {
            let thread = std::thread::current().id();
            let mut js = JsRuntime::new().unwrap();
            js.execute("globalThis.answer = 40 + 2", "init.js").unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            assert_eq!(thread, std::thread::current().id());
            js.execute("if (answer !== 42) throw Error('lost context')", "check.js")
                .unwrap();
            let error = js
                .execute("\nthrow Error('broken')", "failure.js")
                .unwrap_err();
            assert_eq!(error.origin, "failure.js");
            assert_eq!(error.line, Some(2));
            assert!(error.message.contains("broken"));
            assert!(error.stack.as_deref().unwrap().contains("failure.js:2"));
            assert!(js.execute("let = ;", "syntax.js").is_err());
            assert!(
                js.execute("throw Object.create(null)", "object.js")
                    .is_err()
            );
            assert!(
                js.execute("throw { toString() { throw 1 } }", "hostile.js")
                    .is_err()
            );
            js.execute(
                "if (answer !== 42) throw Error('lost context')",
                "recovery.js",
            )
            .unwrap();
        });
    }

    fn tokio_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
    }

    #[test]
    fn async_operations_settle_by_identity_not_registration_order() {
        tokio_runtime().block_on(async {
            let mut js = JsRuntime::new().unwrap();
            assert_eq!(
                js.isolate.get_microtasks_policy(),
                v8::MicrotasksPolicy::Explicit
            );
            js.execute(
                r#"
                globalThis.order = [];
                Rusty.sleep(0).then(() => order.push('first'));
                Rusty.sleep(0).then(() => order.push('second'));
            "#,
                "identity.js",
            )
            .unwrap();
            let (send, receive) = tokio::sync::oneshot::channel();
            {
                let mut state = js.state.borrow_mut();
                state.requests[0].future = Box::pin(async {
                    receive.await.unwrap();
                    Ok(())
                });
                state.requests[1].future = Box::pin(async {
                    send.send(()).unwrap();
                    Ok(())
                });
            }
            js.run_event_loop().await.unwrap();
            assert!(js.state.borrow().operations.is_empty());
            js.execute(
                "if (order.join() !== 'second,first') throw Error(order)",
                "verify.js",
            )
            .unwrap();
        });
    }

    #[test]
    fn host_failures_and_task_panics_reject_their_promises() {
        tokio_runtime().block_on(async {
            let mut js = JsRuntime::new().unwrap();
            for panic in [false, true] {
                js.execute("globalThis.caught = false; Rusty.sleep(0).catch(e => { caught = e instanceof Error; });", "failure.js").unwrap();
                js.state.borrow_mut().requests[0].future = Box::pin(async move {
                    assert!(!panic, "test host panic");
                    Err("test host failure".into())
                });
                js.run_event_loop().await.unwrap();
                assert!(js.state.borrow().operations.is_empty());
                js.execute("if (!caught) throw Error('missing rejection')", "verify.js").unwrap();
            }
            js.execute("Rusty.sleep(0)", "unhandled.js").unwrap();
            js.state.borrow_mut().requests[0].future = Box::pin(async { Err("failure without a catch".into()) });
            let error = js.run_event_loop().await.unwrap_err();
            assert!(error.message.contains("failure without a catch"));
            assert!(js.state.borrow().operations.is_empty());
            js.execute("1 + 1", "recovery.js").unwrap();
        });
    }

    #[test]
    fn cancellation_aborts_tasks_and_releases_roots_before_reuse() {
        struct Dropped(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        tokio_runtime().block_on(async {
            let mut js = JsRuntime::new().unwrap();
            js.execute("Rusty.sleep(2147483647)", "cancel.js").unwrap();
            let old_id = *js.state.borrow().operations.keys().next().unwrap();
            let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let guard = Dropped(dropped.clone());
            js.state.borrow_mut().requests[0].future = Box::pin(async move {
                let _guard = guard;
                std::future::pending::<Result<(), String>>().await
            });
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(10), js.run_event_loop())
                    .await
                    .is_err()
            );
            assert!(js.state.borrow().operations.is_empty());
            tokio::task::yield_now().await;
            assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
            js.execute(
                "globalThis.finished = false; Rusty.sleep(0).then(() => finished = true)",
                "reuse.js",
            )
            .unwrap();
            js.settle(old_id, Ok(())).unwrap();
            js.execute(
                "if (finished) throw Error('stale completion settled new work')",
                "stale.js",
            )
            .unwrap();
            js.run_event_loop().await.unwrap();
            js.execute("if (!finished) throw Error('not reusable')", "verify.js")
                .unwrap();
        });
    }

    #[test]
    fn synchronous_and_microtask_errors_cancel_queued_work() {
        let mut js = JsRuntime::new().unwrap();
        for source in [
            "Rusty.sleep(2147483647); throw Error('stop')",
            "Rusty.sleep(2147483647); Promise.resolve().then(() => { throw Error('stop') })",
        ] {
            assert!(js.execute(source, "stop.js").is_err());
            assert!(js.state.borrow().operations.is_empty());
            assert!(js.state.borrow().requests.is_empty());
        }
        js.execute("Rusty.sleep(2147483647)", "manual.js").unwrap();
        js.cancel_pending();
        assert!(js.state.borrow().operations.is_empty());
    }

    #[test]
    fn callback_diagnostics_use_the_definition_origin() {
        tokio_runtime().block_on(async {
            let mut js = JsRuntime::new().unwrap();
            js.execute(
                "globalThis.fail = () => { throw Error('callback'); }",
                "definition.js",
            )
            .unwrap();
            let error = js.execute("fail()", "caller.js").unwrap_err();
            assert_eq!(error.origin, "definition.js");
            js.execute("Rusty.sleep(0).then(fail)", "async-caller.js")
                .unwrap();
            let error = js.run_event_loop().await.unwrap_err();
            assert_eq!(error.origin, "definition.js");
            js.execute("setTimeout(fail, 0)", "timer-caller.js")
                .unwrap();
            let error = js.run_event_loop().await.unwrap_err();
            assert_eq!(error.origin, "definition.js");
            assert!(js.state.borrow().timers.is_empty());
        });
    }

    #[test]
    fn intervals_skip_missed_ticks_and_cancellation_releases_timer_roots() {
        tokio_runtime().block_on(async {
            tokio::time::pause();
            let mut js = JsRuntime::new().unwrap();
            js.execute(
                "globalThis.ticks = 0; setInterval(() => ++ticks, 10)",
                "interval.js",
            )
            .unwrap();
            tokio::time::advance(std::time::Duration::from_secs(60)).await;
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(1), js.run_event_loop())
                    .await
                    .is_err()
            );
            assert!(js.state.borrow().timers.is_empty());
            js.execute(
                "if (ticks !== 1) throw Error('replayed missed ticks: ' + ticks)",
                "verify.js",
            )
            .unwrap();
            js.execute(
                "setTimeout(() => { throw Error('cancelled callback ran') }, 2147483647)",
                "manual.js",
            )
            .unwrap();
            js.cancel_pending();
            assert!(js.state.borrow().timers.is_empty());
            js.run_event_loop().await.unwrap();
        });
    }

    #[test]
    fn zero_delay_timer_chain_does_not_starve_host_completions() {
        tokio_runtime().block_on(async {
            let mut js = JsRuntime::new().unwrap();
            js.execute(
                r#"
                let timer;
                function again() { timer = setTimeout(again, 0); }
                again();
                Rusty.sleep(0).then(() => clearTimeout(timer));
            "#,
                "fairness.js",
            )
            .unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(1), js.run_event_loop())
                .await
                .unwrap()
                .unwrap();
            assert!(js.state.borrow().timers.is_empty());
            assert!(js.state.borrow().operations.is_empty());
        });
    }

    #[test]
    fn intervals_do_not_await_returned_promises_but_drain_their_host_work() {
        tokio_runtime().block_on(async {
            tokio::time::pause();
            let mut js = JsRuntime::new().unwrap();
            js.execute(r#"
                globalThis.started = 0;
                globalThis.finished = 0;
                globalThis.overlapped = false;
                const interval = setInterval(async () => {
                    if (++started === 2) {
                        overlapped = finished === 0;
                        clearInterval(interval);
                    }
                    await Rusty.sleep(10);
                    ++finished;
                }, 1);
            "#, "async-interval.js").unwrap();
            js.run_event_loop().await.unwrap();
            js.execute("if (started !== 2 || finished !== 2 || !overlapped) throw Error('incorrect async interval lifecycle')", "verify.js").unwrap();
            assert!(js.state.borrow().timers.is_empty());
            assert!(js.state.borrow().operations.is_empty());
        });
    }
}
