//! Cached file and built-in ESM graphs. File reads are synchronous; evaluation may await host work.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use url::Url;

use crate::builtins::{self, Builtin};
use crate::{JsError, JsRuntime, caught_error, message_error};

pub(crate) type SharedModules = Rc<RefCell<ModuleMap>>;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ModuleId {
    File(PathBuf),
    Builtin(Builtin),
}

impl ModuleId {
    fn origin(&self) -> Cow<'_, str> {
        match self {
            Self::File(path) => path.to_string_lossy(),
            Self::Builtin(builtin) => Cow::Borrowed(builtin.specifier()),
        }
    }
}

struct CachedModule {
    module: v8::Global<v8::Module>,
    evaluation: Option<v8::Global<v8::Promise>>,
}

struct ImportRequest {
    resolver: v8::Global<v8::PromiseResolver>,
    specifier: String,
    referrer: String,
}

struct ImportWaiter {
    resolver: v8::Global<v8::PromiseResolver>,
    path: ModuleId,
}

struct LoadFailure {
    diagnostic: JsError,
    exception: Option<v8::Global<v8::Value>>,
}

#[derive(Default)]
pub(crate) struct ModuleMap {
    entries: HashMap<ModuleId, CachedModule>,
    edges: HashMap<(ModuleId, String), ModuleId>,
    active: HashSet<ModuleId>,
    requests: Vec<ImportRequest>,
    waiters: Vec<ImportWaiter>,
    main: Option<ModuleId>,
    pub(crate) native_exports: Option<builtins::NativeExports>,
}

impl ModuleMap {
    pub(crate) fn builtin_for_module(&self, module: v8::Local<v8::Module>) -> Option<Builtin> {
        self.entries.iter().find_map(|(id, entry)| match id {
            ModuleId::Builtin(builtin) if entry.module == module => Some(*builtin),
            _ => None,
        })
    }
    pub(crate) fn is_source_module(&self, scope: &v8::PinScope, path: &Path) -> bool {
        self.entries
            .get(&ModuleId::File(path.to_path_buf()))
            .is_some_and(|entry| v8::Local::new(scope, &entry.module).is_source_text_module())
    }

    pub(crate) fn file_for_module(&self, module: v8::Local<v8::Module>) -> Option<PathBuf> {
        self.entries.iter().find_map(|(id, entry)| match id {
            ModuleId::File(path) if entry.module == module => Some(path.clone()),
            _ => None,
        })
    }
    pub fn cancel(&mut self) {
        self.active.clear();
        self.requests.clear();
        self.waiters.clear();
    }
    pub fn clear(&mut self) {
        self.cancel();
        self.entries.clear();
        self.edges.clear();
        self.main = None;
        self.native_exports = None;
    }

    fn rollback(&mut self, staged: &[ModuleId]) {
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
        let path = path.as_ref();
        let result = canonical_file(path, &path.to_string_lossy())
            .and_then(|path| {
                crate::packages::validate_format(&path, &path.to_string_lossy(), true)?;
                if self.commonjs.borrow().contains(&path) {
                    return Err(JsError::host(
                        "file is already cached as CommonJS; cannot force ES module mode",
                        &path.to_string_lossy(),
                    ));
                }
                Ok(path)
            })
            .map(ModuleId::File)
            .and_then(|entry| {
                self.start_module(entry, true)
                    .map_err(|failure| failure.diagnostic)
            })
            .map(|_| ());
        let checkpoint = self.checkpoint();
        let result = result.and(checkpoint);
        if result.is_err() {
            self.cancel_pending();
        }
        result
    }

    fn start_module(&mut self, entry: ModuleId, watch: bool) -> Result<ModuleId, LoadFailure> {
        let origin = entry.origin().into_owned();
        self.state.borrow_mut().origin = origin.clone();
        v8::scope!(let scope, &mut self.isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        v8::tc_scope!(let scope, scope);
        let mut staged = Vec::new();
        let mut evaluation_started = false;
        let result = (|| {
            load_graph(scope, &self.modules, &entry, &mut staged, watch)?;
            self.state.borrow_mut().origin = entry.origin().into_owned();
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
                return Err(caught_error(scope, &entry.origin()));
            }
            evaluation_started = true;
            if watch && self.modules.borrow().main.is_none() {
                self.modules.borrow_mut().main = Some(entry.clone());
            }
            if !already_started {
                if module.get_status() == v8::ModuleStatus::Errored {
                    scope.throw_exception(module.get_exception());
                    return Err(caught_error(scope, &entry.origin()));
                }
                let value = module
                    .evaluate(scope)
                    .ok_or_else(|| caught_error(scope, &entry.origin()))?;
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
            if watch {
                self.modules.borrow_mut().active.insert(entry.clone());
            }
            Ok(entry)
        })();
        if result.is_err() && !evaluation_started {
            self.modules.borrow_mut().rollback(&staged);
        }
        result.map_err(|diagnostic| LoadFailure {
            diagnostic,
            exception: scope
                .exception()
                .map(|exception| v8::Global::new(scope, exception)),
        })
    }

    /// Drain requests outside V8 callbacks; each caller owns a separate resolver,
    /// while evaluation and namespace identity belong to the shared module cache.
    pub(crate) fn progress_imports(&mut self, idle: bool) -> Result<bool, JsError> {
        let requests = std::mem::take(&mut self.modules.borrow_mut().requests);
        let mut progressed = !requests.is_empty();
        for request in requests {
            let outcome = resolve_specifier(&request.specifier, &request.referrer)
                .map_err(|diagnostic| LoadFailure {
                    diagnostic,
                    exception: None,
                })
                .and_then(|path| self.start_module(path, false));
            match outcome {
                Ok(path) => self.modules.borrow_mut().waiters.push(ImportWaiter {
                    resolver: request.resolver,
                    path,
                }),
                Err(failure) => {
                    v8::scope!(let scope, &mut self.isolate);
                    let context = v8::Local::new(scope, &self.context);
                    let scope = &mut v8::ContextScope::new(scope, context);
                    v8::tc_scope!(let scope, scope);
                    let resolver = v8::Local::new(scope, &request.resolver);
                    let reason = match failure.exception {
                        Some(exception) => v8::Local::new(scope, exception),
                        None => {
                            let message = v8::String::new(scope, &failure.diagnostic.to_string())
                                .ok_or_else(|| {
                                JsError::host("could not allocate import error", &request.referrer)
                            })?;
                            v8::Exception::type_error(scope, message)
                        }
                    };
                    if resolver.reject(scope, reason) != Some(true) {
                        return Err(caught_error(scope, &request.referrer));
                    }
                }
            }
        }
        let waiters = std::mem::take(&mut self.modules.borrow_mut().waiters);
        let idle = idle
            && self.state.borrow().requests.is_empty()
            && self.state.borrow().timers.is_empty();
        v8::scope!(let scope, &mut self.isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        v8::tc_scope!(let scope, scope);
        // Ready settlements may unblock other evaluations at the next checkpoint.
        // At true idle reject only the newest blocked import, giving an inner
        // catch a turn to recover before rejecting its waiting importers.
        let idle_target = if idle
            && waiters.iter().all(|waiter| {
                let modules = self.modules.borrow();
                modules
                    .entries
                    .get(&waiter.path)
                    .and_then(|entry| entry.evaluation.as_ref())
                    .is_some_and(|promise| {
                        v8::Local::new(scope, promise).state() == v8::PromiseState::Pending
                    })
            }) {
            waiters.len().checked_sub(1)
        } else {
            None
        };
        for (index, waiter) in waiters.into_iter().enumerate() {
            let (module, promise) = {
                let modules = self.modules.borrow();
                let cached = modules
                    .entries
                    .get(&waiter.path)
                    .ok_or_else(|| JsError::host("missing imported module", "<host>"))?;
                let promise = cached
                    .evaluation
                    .as_ref()
                    .ok_or_else(|| JsError::host("missing import evaluation", "<host>"))?;
                (
                    v8::Local::new(scope, &cached.module),
                    v8::Local::new(scope, promise),
                )
            };
            let resolver = v8::Local::new(scope, &waiter.resolver);
            let settled = match promise.state() {
                v8::PromiseState::Fulfilled => {
                    resolver.resolve(scope, module.get_module_namespace())
                }
                v8::PromiseState::Rejected => resolver.reject(scope, promise.result(scope)),
                v8::PromiseState::Pending if idle_target == Some(index) => {
                    let message = v8::String::new(
                        scope,
                        "module evaluation is pending with no active host work",
                    )
                    .ok_or_else(|| JsError::host("could not allocate import error", "<host>"))?;
                    let reason = v8::Exception::type_error(scope, message);
                    resolver.reject(scope, reason)
                }
                v8::PromiseState::Pending => {
                    self.modules.borrow_mut().waiters.push(waiter);
                    continue;
                }
            };
            progressed = true;
            if settled != Some(true) {
                return Err(caught_error(scope, &waiter.path.origin()));
            }
        }
        Ok(progressed)
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
            .ok_or_else(|| JsError::host("missing module evaluation Promise", &path.origin()))?;
            match promise.state() {
                v8::PromiseState::Fulfilled => {
                    self.modules.borrow_mut().active.remove(&path);
                }
                v8::PromiseState::Rejected => {
                    let message = v8::Exception::create_message(scope, promise.result(scope));
                    let mut error = message_error(scope, message, &path.origin());
                    error.message = format!("Module evaluation failed: {}", error.message);
                    return Err(error);
                }
                v8::PromiseState::Pending if idle => {
                    return Err(JsError::host(
                        "module evaluation is pending with no active host work",
                        &path.origin(),
                    ));
                }
                v8::PromiseState::Pending => {}
            }
        }
        Ok(())
    }
}

pub(crate) fn canonical_file(path: &Path, origin: &str) -> Result<PathBuf, JsError> {
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

fn resolve_specifier(specifier: &str, referrer: &str) -> Result<ModuleId, JsError> {
    let fail = |message: &str| JsError::host(message, referrer);
    if specifier.starts_with("rustyjs:") {
        return Builtin::resolve(specifier)
            .map(ModuleId::Builtin)
            .ok_or_else(|| fail(&format!("unknown built-in module {specifier:?}")));
    }
    let path = Path::new(specifier);
    let url = if specifier.starts_with("file:") {
        Url::parse(specifier).map_err(|_| fail("invalid file module URL"))?
    } else if path.is_absolute() {
        // Native absolute paths remain supported, including Windows drive paths.
        Url::from_file_path(path).map_err(|_| fail("invalid absolute module path"))?
    } else if specifier.starts_with("./") || specifier.starts_with("../") {
        if referrer == "<eval>" {
            return Err(fail("relative imports require a file referrer"));
        }
        let base = canonical_file(Path::new(referrer), referrer)?;
        Url::from_file_path(base)
            .map_err(|_| fail("invalid module referrer path"))?
            .join(specifier)
            .map_err(|_| fail("invalid relative module URL"))?
    } else {
        return crate::packages::resolve(specifier, referrer);
    };
    if url.scheme() != "file" || url.query().is_some() || url.fragment().is_some() {
        return Err(fail(
            "module URLs must use file: without a query or fragment",
        ));
    }
    // URL decoding must not turn escaped separators into additional path segments.
    let encoded = url.path().to_ascii_lowercase();
    if encoded.contains("%2f") || encoded.contains("%5c") {
        return Err(fail(
            "encoded path separators are not supported in module URLs",
        ));
    }
    let path = url
        .to_file_path()
        .map_err(|_| fail("file URL cannot be converted to a local path"))?;
    let path = canonical_file(&path, referrer)?;
    crate::packages::validate_format(&path, referrer, false)?;
    Ok(ModuleId::File(path))
}

pub(crate) fn import_dynamic<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    _options: v8::Local<'s, v8::Data>,
    resource: v8::Local<'s, v8::Value>,
    specifier: v8::Local<'s, v8::String>,
    attributes: v8::Local<'s, v8::FixedArray>,
) -> Option<v8::Local<'s, v8::Promise>> {
    let resolver = v8::PromiseResolver::new(scope)?;
    let promise = resolver.get_promise(scope);
    if attributes.length() != 0 {
        let message = v8::String::new(scope, "import attributes are not supported")?;
        let reason = v8::Exception::type_error(scope, message);
        resolver.reject(scope, reason)?;
        return Some(promise);
    }
    let referrer = resource.to_rust_string_lossy(scope);
    let specifier = specifier.to_rust_string_lossy(scope);
    let modules = scope.get_slot::<SharedModules>()?.clone();
    modules.borrow_mut().requests.push(ImportRequest {
        resolver: v8::Global::new(scope, resolver),
        specifier,
        referrer,
    });
    Some(promise)
}

pub(crate) fn import_with_phase<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    options: v8::Local<'s, v8::Data>,
    resource: v8::Local<'s, v8::Value>,
    specifier: v8::Local<'s, v8::String>,
    phase: v8::ModuleImportPhase,
    attributes: v8::Local<'s, v8::FixedArray>,
) -> Option<v8::Local<'s, v8::Promise>> {
    if phase == v8::ModuleImportPhase::kEvaluation {
        return import_dynamic(scope, options, resource, specifier, attributes);
    }
    let resolver = v8::PromiseResolver::new(scope)?;
    let message = v8::String::new(scope, "non-evaluation import phases are not supported")?;
    let reason = v8::Exception::type_error(scope, message);
    resolver.reject(scope, reason)?;
    Some(resolver.get_promise(scope))
}

pub(crate) extern "C" fn initialize_meta(
    context: v8::Local<v8::Context>,
    module: v8::Local<v8::Module>,
    meta: v8::Local<v8::Object>,
) {
    // SAFETY: V8 supplies the live context, module and metadata on its owner thread.
    v8::callback_scope!(unsafe scope, context);
    let Some(modules) = scope.get_slot::<SharedModules>().cloned() else {
        return;
    };
    let (url, main) = {
        let modules = modules.borrow();
        let Some((path, _)) = modules
            .entries
            .iter()
            .find(|(_, cached)| cached.module == module)
        else {
            return;
        };
        let ModuleId::File(file) = path else {
            return;
        };
        let Ok(url) = Url::from_file_path(file) else {
            return;
        };
        (url.to_string(), modules.main.as_ref() == Some(path))
    };
    let Some(key) = v8::String::new(scope, "url") else {
        return;
    };
    let Some(value) = v8::String::new(scope, &url) else {
        return;
    };
    if meta.create_data_property(scope, key.into(), value.into()) != Some(true) {
        return;
    }
    let Some(key) = v8::String::new(scope, "main") else {
        return;
    };
    let value = v8::Boolean::new(scope, main);
    meta.create_data_property(scope, key.into(), value.into());
}

fn load_graph(
    scope: &mut v8::PinnedRef<'_, v8::TryCatch<v8::HandleScope>>,
    modules: &SharedModules,
    entry: &ModuleId,
    staged: &mut Vec<ModuleId>,
    force_entry_esm: bool,
) -> Result<(), JsError> {
    // An explicit worklist avoids recursive Rust stack growth and permits cycles.
    let mut pending = vec![entry.clone()];
    while let Some(path) = pending.pop() {
        if modules.borrow().entries.contains_key(&path) {
            continue;
        }
        let origin = path.origin();
        let file = match &path {
            ModuleId::File(file) => file,
            ModuleId::Builtin(builtin) => {
                let module = builtins::create(scope, *builtin)
                    .ok_or_else(|| JsError::host("could not create built-in module", &origin))?;
                modules.borrow_mut().entries.insert(
                    path.clone(),
                    CachedModule {
                        module: v8::Global::new(scope, module),
                        evaluation: None,
                    },
                );
                staged.push(path);
                continue;
            }
        };
        if !(force_entry_esm && path == *entry)
            && crate::packages::format(file, &origin)? == crate::packages::Format::CommonJs
        {
            let module = crate::commonjs::create_bridge(scope, file)
                .ok_or_else(|| JsError::host("could not create CommonJS import bridge", &origin))?;
            modules.borrow_mut().entries.insert(
                path.clone(),
                CachedModule {
                    module: v8::Global::new(scope, module),
                    evaluation: None,
                },
            );
            staged.push(path);
            continue;
        }
        let source = std::fs::read_to_string(file)
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
                if request.get_import_attributes().length() != 0
                    || request.get_phase() != v8::ModuleImportPhase::kEvaluation
                {
                    return Err(JsError::host(
                        "import attributes and non-evaluation import phases are not supported",
                        &origin,
                    ));
                }
                resolve_specifier(&specifier, &origin)
            })()
            .map_err(|mut error| {
                error.message = format!("import {specifier:?}: {}", error.message);
                error.line = usize::try_from(location.get_line_number())
                    .ok()
                    .and_then(|line| line.checked_add(1));
                error.column = usize::try_from(location.get_column_number())
                    .ok()
                    .and_then(|column| column.checked_add(1));
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

    #[test]
    fn dynamic_import_cancellation_releases_waiters_and_context_can_continue() {
        runtime().block_on(async {
            let fixture = Fixture::new();
            let path = fixture.write("main.js", "import('./pending.mjs').then(() => globalThis.unexpected = true);");
            fixture.write("pending.mjs", "await Rusty.sleep(2147483647);");
            let mut js = JsRuntime::new().unwrap();
            js.execute(&std::fs::read_to_string(&path).unwrap(), path.to_str().unwrap()).unwrap();
            assert_eq!(js.modules.borrow().waiters.len(), 1);
            assert!(tokio::time::timeout(std::time::Duration::from_millis(10), js.run_event_loop()).await.is_err());
            assert!(js.modules.borrow().waiters.is_empty());
            assert!(js.modules.borrow().requests.is_empty());
            assert!(js.state.borrow().operations.is_empty());
            js.execute("if (globalThis.unexpected) throw Error('cancelled import resumed'); globalThis.reused = true;", "verify.js").unwrap();
            js.run_event_loop().await.unwrap();
        });
    }

    #[test]
    fn file_urls_work_for_static_and_inline_imports_with_encoded_names() {
        runtime().block_on(async {
            let fixture = Fixture::new();
            let dependency = fixture.write("space # café.mjs", "globalThis.urlRuns = (globalThis.urlRuns ?? 0) + 1; export const value = 42;");
            let url = Url::from_file_path(&dependency).unwrap();
            let path = fixture.write("main.mjs", &format!("import {{value}} from '{url}'; if (value !== 42) throw Error('file URL');"));
            let mut js = JsRuntime::new().unwrap();
            js.execute_module(path).unwrap();
            js.run_event_loop().await.unwrap();
            js.execute(&format!("import('{url}').then(module => {{ if (module.value !== 42 || urlRuns !== 1) throw Error('inline URL import'); }});"), "<eval>").unwrap();
            js.run_event_loop().await.unwrap();
        });
    }

    #[test]
    fn builtin_cache_survives_cancellation_and_globals_are_not_export_backing() {
        runtime().block_on(async {
            let mut js = JsRuntime::new().unwrap();
            js.execute("import('rustyjs:timers').then(async timers => { globalThis.nativeTimers = timers; await timers.sleep(2147483647); globalThis.unexpected = true; });", "<eval>").unwrap();
            assert!(tokio::time::timeout(std::time::Duration::from_millis(10), js.run_event_loop()).await.is_err());
            assert!(js.state.borrow().operations.is_empty());
            assert!(js.modules.borrow().native_exports.is_some());
            js.execute("globalThis.Rusty = null; globalThis.setTimeout = null; import('rustyjs:timers').then(async timers => { if (timers !== nativeTimers || globalThis.unexpected) throw Error('lost cached native module'); await timers.sleep(0); await new Promise(resolve => timers.setTimeout(resolve, 0)); });", "<eval>").unwrap();
            js.run_event_loop().await.unwrap();
        });
    }

    #[test]
    fn failed_package_preparation_can_retry_in_the_same_runtime() {
        runtime().block_on(async {
            let fixture = Fixture::new();
            let package = fixture.0.join("node_modules/retry");
            std::fs::create_dir_all(&package).unwrap();
            std::fs::write(package.join("package.json"), "{").unwrap();
            std::fs::write(
                package.join("index.mjs"),
                "await Rusty.sleep(0); export const value = 42;",
            )
            .unwrap();
            let entry = fixture.write(
                "main.mjs",
                "import {value} from 'retry'; globalThis.packageResult = value;",
            );
            let mut js = JsRuntime::new().unwrap();
            assert!(
                js.execute_module(&entry)
                    .unwrap_err()
                    .message
                    .contains("package.json")
            );
            assert!(js.modules.borrow().entries.is_empty());
            std::fs::write(package.join("package.json"), r#"{"exports":"./index.mjs"}"#).unwrap();
            js.execute_module(&entry).unwrap();
            js.run_event_loop().await.unwrap();
            js.execute(
                "if (packageResult !== 42) throw Error('package retry failed');",
                "verify.js",
            )
            .unwrap();
        });
    }

    #[test]
    fn commonjs_cache_survives_cancellation_and_failed_modules_can_be_repaired() {
        runtime().block_on(async {
            let fixture = Fixture::new();
            let path = fixture.write("cached.cjs", "globalThis.commonJsRuns = (globalThis.commonJsRuns ?? 0) + 1; setTimeout(() => globalThis.unexpected = true, 2147483647); module.exports = 42; globalThis.savedRequire = require;");
            let mut js = JsRuntime::new().unwrap();
            js.execute_commonjs(&path).unwrap();
            assert!(tokio::time::timeout(std::time::Duration::from_millis(10), js.run_event_loop()).await.is_err());
            assert!(js.state.borrow().timers.is_empty());
            assert!(js.commonjs.borrow().contains(&std::fs::canonicalize(&path).unwrap()));
            fixture.write("cached.cjs", "throw Error('cached source re-executed');");
            js.execute_commonjs(&path).unwrap();
            js.execute("if (commonJsRuns !== 1 || globalThis.unexpected || savedRequire('./cached.cjs') !== 42 || typeof require !== 'undefined') throw Error('CJS cancellation/reuse');", "verify.js").unwrap();
            let bad = fixture.write("failed.cjs", "throw Error('retry source');");
            assert!(js.execute_commonjs(&bad).unwrap_err().message.contains("retry source"));
            assert!(!js.commonjs.borrow().contains(&std::fs::canonicalize(&bad).unwrap()));
            fixture.write("failed.cjs", "globalThis.repaired = 42;");
            js.execute_commonjs(bad).unwrap();
            js.execute("if (repaired !== 42) throw Error('CJS repair failed');", "verify.js").unwrap();
            js.run_event_loop().await.unwrap();
        });
    }

    #[test]
    fn forced_esm_and_commonjs_cannot_reinterpret_an_already_cached_file() {
        runtime().block_on(async {
            let fixture = Fixture::new();
            let esm = fixture.write("forced.js", "export const value = 42;");
            let cjs = fixture.write("cached.js", "module.exports = 42;");
            let mut js = JsRuntime::new().unwrap();
            js.execute_module(&esm).unwrap();
            assert!(
                js.execute_commonjs(esm)
                    .unwrap_err()
                    .message
                    .contains("use import()")
            );
            js.execute_commonjs(&cjs).unwrap();
            assert!(
                js.execute_module(cjs)
                    .unwrap_err()
                    .message
                    .contains("already cached as CommonJS")
            );
            js.run_event_loop().await.unwrap();
        });
    }
}
