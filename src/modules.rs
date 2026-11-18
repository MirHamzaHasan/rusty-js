//! Initial local-file ESM graph. File loading is synchronous; evaluation may await host work.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::{JsError, JsRuntime, caught_error, message_error};

pub(crate) type SharedModules = Rc<RefCell<ModuleMap>>;

struct CachedModule {
    module: v8::Global<v8::Module>,
    evaluation: Option<v8::Global<v8::Promise>>,
}

#[derive(Default)]
pub(crate) struct ModuleMap {
    entries: HashMap<PathBuf, CachedModule>,
    edges: HashMap<(PathBuf, String), PathBuf>,
    active: HashSet<PathBuf>,
}

impl ModuleMap {
    pub fn cancel(&mut self) {
        self.active.clear();
    }
    pub fn clear(&mut self) {
        self.active.clear();
        self.entries.clear();
        self.edges.clear();
    }

    fn rollback(&mut self, staged: &[PathBuf]) {
        for path in staged {
            self.entries.remove(path);
        }
        self.edges
            .retain(|(referrer, _), _| !staged.contains(referrer));
    }
}

pub(crate) struct EvaluationGuard(pub SharedModules);
impl Drop for EvaluationGuard {
    fn drop(&mut self) {
        self.0.borrow_mut().cancel();
    }
}

impl JsRuntime {
    /// Start a local-file ESM graph, then drain its initial microtasks.
    /// Call run_event_loop to finish host-backed top-level await and callback work.
    pub fn execute_module(&mut self, path: impl AsRef<Path>) -> Result<(), JsError> {
        let result = self.start_module(path.as_ref());
        let checkpoint = self.checkpoint();
        let result = result.and(checkpoint);
        if result.is_err() {
            self.cancel_pending();
        }
        result
    }

    fn start_module(&mut self, path: &Path) -> Result<(), JsError> {
        let origin = path.to_string_lossy().into_owned();
        self.state.borrow_mut().origin = origin.clone();
        v8::scope!(let scope, &mut self.isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        v8::tc_scope!(let scope, scope);
        let mut staged = Vec::new();
        let mut evaluation_started = false;
        let result = (|| {
            let entry = canonical_file(path, &origin)?;
            load_graph(scope, &self.modules, &entry, &mut staged)?;
            self.state.borrow_mut().origin = entry.to_string_lossy().into_owned();
            let (module, already_started) = {
                let modules = self.modules.borrow();
                let cached = modules
                    .entries
                    .get(&entry)
                    .ok_or_else(|| JsError::host("entry module was not cached", &origin))?;
                (
                    v8::Local::new(scope, &cached.module),
                    cached.evaluation.is_some(),
                )
            };
            if module.get_status() == v8::ModuleStatus::Uninstantiated
                && module.instantiate_module(scope, resolve) != Some(true)
            {
                return Err(caught_error(scope, &entry.to_string_lossy()));
            }
            evaluation_started = true;
            if !already_started {
                if module.get_status() == v8::ModuleStatus::Errored {
                    let message = v8::Exception::create_message(scope, module.get_exception());
                    return Err(message_error(scope, message, &entry.to_string_lossy()));
                }
                let value = module
                    .evaluate(scope)
                    .ok_or_else(|| caught_error(scope, &entry.to_string_lossy()))?;
                let promise = v8::Local::<v8::Promise>::try_from(value).map_err(|_| {
                    JsError::host("V8 did not return a module evaluation Promise", &origin)
                })?;
                // The host observes this Promise directly, rather than treating it
                // as an unhandled guest-created Promise.
                promise.mark_as_handled();
                self.state
                    .borrow_mut()
                    .rejections
                    .retain(|entry| entry.promise != promise);
                self.modules
                    .borrow_mut()
                    .entries
                    .get_mut(&entry)
                    .ok_or_else(|| JsError::host("entry module was not cached", &origin))?
                    .evaluation = Some(v8::Global::new(scope, promise));
            }
            self.modules.borrow_mut().active.insert(entry);
            Ok(())
        })();
        if result.is_err() && !evaluation_started {
            self.modules.borrow_mut().rollback(&staged);
        }
        result
    }

    pub(crate) fn check_modules(&mut self, idle: bool) -> Result<(), JsError> {
        let active = self
            .modules
            .borrow()
            .active
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        if active.is_empty() {
            return Ok(());
        }
        v8::scope!(let scope, &mut self.isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        for path in active {
            let promise = {
                let modules = self.modules.borrow();
                modules
                    .entries
                    .get(&path)
                    .and_then(|entry| entry.evaluation.as_ref())
                    .map(|promise| v8::Local::new(scope, promise))
            }
            .ok_or_else(|| {
                JsError::host("missing module evaluation Promise", &path.to_string_lossy())
            })?;
            match promise.state() {
                v8::PromiseState::Fulfilled => {
                    self.modules.borrow_mut().active.remove(&path);
                }
                v8::PromiseState::Rejected => {
                    let message = v8::Exception::create_message(scope, promise.result(scope));
                    let mut error = message_error(scope, message, &path.to_string_lossy());
                    error.message = format!("Module evaluation failed: {}", error.message);
                    return Err(error);
                }
                v8::PromiseState::Pending if idle => {
                    return Err(JsError::host(
                        "module evaluation is pending with no active host work",
                        &path.to_string_lossy(),
                    ));
                }
                v8::PromiseState::Pending => {}
            }
        }
        Ok(())
    }
}

fn canonical_file(path: &Path, origin: &str) -> Result<PathBuf, JsError> {
    let path = std::fs::canonicalize(path).map_err(|error| {
        JsError::host(
            &format!("cannot load module {}: {error}", path.display()),
            origin,
        )
    })?;
    if !path.is_file() {
        return Err(JsError::host(
            &format!("module is not a regular file: {}", path.display()),
            origin,
        ));
    }
    Ok(path)
}

fn load_graph(
    scope: &mut v8::PinnedRef<'_, v8::TryCatch<v8::HandleScope>>,
    modules: &SharedModules,
    entry: &Path,
    staged: &mut Vec<PathBuf>,
) -> Result<(), JsError> {
    // An explicit worklist avoids recursive Rust stack growth and permits cycles.
    let mut pending = vec![entry.to_path_buf()];
    while let Some(path) = pending.pop() {
        if modules.borrow().entries.contains_key(&path) {
            continue;
        }
        let origin = path.to_string_lossy();
        let source = std::fs::read_to_string(&path)
            .map_err(|error| JsError::host(&format!("cannot read module: {error}"), &origin))?;
        let source = v8::String::new(scope, &source)
            .ok_or_else(|| JsError::host("could not allocate module source", &origin))?;
        let filename = v8::String::new(scope, &origin)
            .ok_or_else(|| JsError::host("could not allocate module filename", &origin))?;
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
            true,
            None,
        );
        let mut source = v8::script_compiler::Source::new(source, Some(&script_origin));
        let module = v8::script_compiler::compile_module(scope, &mut source)
            .ok_or_else(|| caught_error(scope, &origin))?;
        modules.borrow_mut().entries.insert(
            path.clone(),
            CachedModule {
                module: v8::Global::new(scope, module),
                evaluation: None,
            },
        );
        staged.push(path.clone());
        let requests = module.get_module_requests();
        for index in 0..requests.length() {
            let request = requests
                .get(scope, index)
                .and_then(|value| v8::Local::<v8::ModuleRequest>::try_from(value).ok())
                .ok_or_else(|| JsError::host("invalid V8 module request", &origin))?;
            let specifier = request.get_specifier().to_rust_string_lossy(scope);
            let location = module.source_offset_to_location(request.get_source_offset());
            let resolved = (|| {
                if request.get_import_attributes().length() != 0 || request.get_phase() != v8::ModuleImportPhase::kEvaluation {
                    return Err(JsError::host("import attributes and non-evaluation import phases are not supported", &origin));
                }
                let import = Path::new(&specifier);
                if !specifier.starts_with("./") && !specifier.starts_with("../") && !import.is_absolute() {
                    return Err(JsError::host(&format!("unsupported module specifier {specifier:?}; use an explicit local file path"), &origin));
                }
                let parent = path.parent().ok_or_else(|| JsError::host("module has no parent directory", &origin))?;
                canonical_file(&parent.join(import), &origin)
            })().map_err(|mut error| {
                error.message = format!("import {specifier:?}: {}", error.message);
                error.line = usize::try_from(location.get_line_number()).ok().and_then(|line| line.checked_add(1));
                error.column = usize::try_from(location.get_column_number()).ok().and_then(|column| column.checked_add(1));
                error
            })?;
            modules
                .borrow_mut()
                .edges
                .insert((path.clone(), specifier), resolved.clone());
            pending.push(resolved);
        }
    }
    Ok(())
}

fn resolve<'s>(
    context: v8::Local<'s, v8::Context>,
    specifier: v8::Local<'s, v8::String>,
    _attributes: v8::Local<'s, v8::FixedArray>,
    referrer: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Module>> {
    // SAFETY: called by V8 during linking with its live context on the owner thread.
    v8::callback_scope!(unsafe scope, context);
    let state = scope.get_slot::<SharedModules>()?.clone();
    let specifier = specifier.to_rust_string_lossy(scope);
    let module = {
        let modules = state.borrow();
        let path = modules
            .entries
            .iter()
            .find(|(_, entry)| entry.module == referrer)
            .map(|(path, _)| path);
        path.and_then(|path| modules.edges.get(&(path.clone(), specifier)))
            .and_then(|path| modules.entries.get(path))
            .map(|entry| v8::Local::new(scope, &entry.module))
    };
    if module.is_none()
        && let Some(message) = v8::String::new(scope, "module dependency was not prepared")
    {
        let error = v8::Exception::error(scope, message);
        scope.throw_exception(error);
    }
    module
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rustyjs-modules-{}-{nanos}-{}",
                std::process::id(),
                NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn write(&self, name: &str, source: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, source).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
    }

    #[test]
    fn cache_preserves_identity_results_and_evaluation_failures() {
        runtime().block_on(async {
            let fixture = Fixture::new();
            let path = fixture.write("cached.mjs", "globalThis.runs = (globalThis.runs ?? 0) + 1; export const value = 42;");
            let mut js = JsRuntime::new().unwrap();
            js.execute_module(&path).unwrap();
            js.run_event_loop().await.unwrap();
            fixture.write("cached.mjs", "throw Error('changed source should stay cached')");
            js.execute_module(&path).unwrap();
            js.run_event_loop().await.unwrap();
            #[cfg(unix)]
            {
                let alias = fixture.0.join("alias.mjs");
                std::os::unix::fs::symlink(&path, &alias).unwrap();
                js.execute_module(alias).unwrap();
                js.run_event_loop().await.unwrap();
            }
            js.execute("if (runs !== 1) throw Error('evaluated twice')", "verify.js").unwrap();
            let bad = fixture.write("failure.mjs", "globalThis.failures = (globalThis.failures ?? 0) + 1; throw Error('cached failure');");
            assert!(js.execute_module(&bad).unwrap_err().message.contains("cached failure"));
            fixture.write("failure.mjs", "globalThis.failures = 99");
            assert!(js.execute_module(&bad).unwrap_err().message.contains("cached failure"));
            js.execute("if (failures !== 1) throw Error('retried failed evaluation')", "verify.js").unwrap();
        });
    }

    #[test]
    fn failed_compilation_and_linking_roll_back_new_cache_entries() {
        runtime().block_on(async {
            let fixture = Fixture::new();
            let path = fixture.write(
                "main.mjs",
                "import { missing } from './dependency.mjs'; globalThis.result = missing;",
            );
            fixture.write("dependency.mjs", "export const = ;");
            let mut js = JsRuntime::new().unwrap();
            assert!(js.execute_module(&path).is_err());
            assert!(js.modules.borrow().entries.is_empty());
            fixture.write("dependency.mjs", "export const answer = 42;");
            assert!(js.execute_module(&path).is_err());
            assert!(js.modules.borrow().entries.is_empty());
            fixture.write(
                "main.mjs",
                "import { answer } from './dependency.mjs'; globalThis.result = answer;",
            );
            js.execute_module(path).unwrap();
            js.run_event_loop().await.unwrap();
            js.execute(
                "if (result !== 42) throw Error('failed recovery')",
                "verify.js",
            )
            .unwrap();
        });
    }

    #[test]
    fn cancelling_module_work_preserves_a_reusable_context_and_pending_identity() {
        runtime().block_on(async {
            let fixture = Fixture::new();
            let path = fixture.write(
                "pending.mjs",
                "await Rusty.sleep(2147483647); globalThis.unexpected = true;",
            );
            let mut js = JsRuntime::new().unwrap();
            js.execute_module(&path).unwrap();
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(10), js.run_event_loop())
                    .await
                    .is_err()
            );
            assert!(js.modules.borrow().active.is_empty());
            assert!(js.state.borrow().operations.is_empty());
            js.execute(
                "if (globalThis.unexpected) throw Error('cancelled module resumed')",
                "verify.js",
            )
            .unwrap();
            js.execute_module(path).unwrap();
            let error = js.run_event_loop().await.unwrap_err();
            assert!(error.message.contains("pending with no active host work"));
        });
    }
}
