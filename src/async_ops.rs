//! Isolate-local Promise roots and Rust-only work requests.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::time::Duration;

pub(crate) type SharedState = Rc<RefCell<HostState>>;
pub(crate) type HostFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

pub(crate) struct Request {
    pub id: u64,
    pub future: HostFuture,
}

pub(crate) struct Operation {
    pub resolver: v8::Global<v8::PromiseResolver>,
    pub origin: String,
}

pub(crate) struct Rejection {
    pub promise: v8::Global<v8::Promise>,
    pub reason: v8::Global<v8::Value>,
    pub origin: String,
}

#[derive(Default)]
pub(crate) struct HostState {
    pub origin: String,
    pub operations: HashMap<u64, Operation>,
    pub requests: Vec<Request>,
    pub rejections: Vec<Rejection>,
    pub timers: HashMap<u64, crate::timers::Timer>,
    next_id: u64,
}

impl HostState {
    pub fn allocate_id(&mut self) -> Option<u64> {
        // Callback timer IDs are JavaScript Numbers; never round or reuse an ID.
        let id = self
            .next_id
            .checked_add(1)
            .filter(|id| *id <= 9_007_199_254_740_991)?;
        self.next_id = id;
        Some(id)
    }

    pub fn cancel(&mut self) {
        self.requests.clear();
        self.operations.clear();
        self.rejections.clear();
        self.timers.clear();
    }
}

/// Dropping the event-loop future releases roots even when it is cancelled.
pub(crate) struct LoopGuard(pub SharedState);

impl Drop for LoopGuard {
    fn drop(&mut self) {
        self.0.borrow_mut().cancel();
    }
}

pub(crate) fn install(scope: &mut v8::PinScope, context: v8::Local<v8::Context>) -> Option<()> {
    let namespace = v8::Object::new(scope);
    let sleep = v8::Function::new(scope, sleep)?;
    let sleep_key = v8::String::new(scope, "sleep")?;
    let namespace_key = v8::String::new(scope, "Rusty")?;
    if namespace.set(scope, sleep_key.into(), sleep.into()) != Some(true)
        || context
            .global(scope)
            .set(scope, namespace_key.into(), namespace.into())
            != Some(true)
    {
        return None;
    }
    Some(())
}

fn sleep(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    // Read only an actual JS Number; validation must not invoke valueOf/toString.
    let milliseconds = v8::Local::<v8::Number>::try_from(args.get(0))
        .ok()
        .map(|value| value.value());
    if !milliseconds.is_some_and(|ms| {
        ms.is_finite() && ms.fract() == 0.0 && (0.0..=2_147_483_647.0).contains(&ms)
    }) {
        if let Some(message) = v8::String::new(
            scope,
            "Rusty.sleep requires an integer from 0 to 2147483647 milliseconds",
        ) {
            let error = v8::Exception::type_error(scope, message);
            scope.throw_exception(error);
        }
        return;
    }
    let Some(resolver) = v8::PromiseResolver::new(scope) else {
        return;
    };
    let promise = resolver.get_promise(scope);
    let resolver = v8::Global::new(scope, resolver);
    let Some(state) = scope.get_slot::<SharedState>().cloned() else {
        return;
    };
    let mut state = state.borrow_mut();
    let Some(id) = state.allocate_id() else {
        drop(state);
        if let Some(message) = v8::String::new(scope, "host operation IDs exhausted") {
            let error = v8::Exception::range_error(scope, message);
            scope.throw_exception(error);
        }
        return;
    };
    let duration = Duration::from_millis(milliseconds.unwrap_or_default() as u64);
    let deadline = tokio::time::Instant::now() + duration;
    let origin = state.origin.clone();
    state.operations.insert(id, Operation { resolver, origin });
    state.requests.push(Request {
        id,
        future: Box::pin(async move {
            tokio::time::sleep_until(deadline).await;
            Ok(())
        }),
    });
    result.set(promise.into());
}

pub(crate) extern "C" fn promise_rejected(message: v8::PromiseRejectMessage) {
    // SAFETY: V8 invokes this callback on the isolate thread with a live message.
    v8::callback_scope!(unsafe scope, &message);
    let Some(state) = scope.get_slot::<SharedState>().cloned() else {
        return;
    };
    let promise = message.get_promise();
    match message.get_event() {
        v8::PromiseRejectEvent::PromiseRejectWithNoHandler => {
            let Some(reason) = message.get_value() else {
                return;
            };
            let promise = v8::Global::new(scope, promise);
            let reason = v8::Global::new(scope, reason);
            let mut state = state.borrow_mut();
            let origin = state.origin.clone();
            state.rejections.push(Rejection {
                promise,
                reason,
                origin,
            });
        }
        v8::PromiseRejectEvent::PromiseHandlerAddedAfterReject => {
            // Identity is checked on the isolate thread, without executing JS.
            state
                .borrow_mut()
                .rejections
                .retain(|entry| entry.promise != promise);
        }
        _ => {}
    }
}
