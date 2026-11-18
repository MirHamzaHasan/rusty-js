//! Synchronous CommonJS loading. All cache roots and callbacks stay on the isolate thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use url::Url;

use crate::async_ops::SharedState;
use crate::builtins::{self, Builtin};
use crate::modules::{ModuleId, SharedModules, canonical_file};
use crate::packages::{self, Format, LoadMode};
use crate::{JsError, JsRuntime, caught_error};

pub(crate) type SharedCommonJs = Rc<RefCell<CommonJsState>>;

#[derive(Default)]
pub(crate) struct CommonJsState {
    cache: HashMap<PathBuf, v8::Global<v8::Object>>,
    builtins: HashMap<Builtin, v8::Global<v8::Object>>,
    main: Option<v8::Global<v8::Object>>,
    depth: usize,
}

impl CommonJsState {
    pub fn clear(&mut self) {
        self.cache.clear();
        self.builtins.clear();
        self.main = None;
    }
    pub fn contains(&self, path: &Path) -> bool {
        self.cache.contains_key(path)
    }
}

struct DepthGuard(SharedCommonJs);
impl Drop for DepthGuard {
    fn drop(&mut self) {
        self.0.borrow_mut().depth -= 1;
    }
}

struct LoadGuard {
    state: SharedCommonJs,
    path: PathBuf,
    main: bool,
    success: bool,
}
impl Drop for LoadGuard {
    fn drop(&mut self) {
        if !self.success {
            let mut state = self.state.borrow_mut();
            state.cache.remove(&self.path);
            if self.main {
                state.main = None;
            }
        }
    }
}

struct OriginGuard(SharedState, String);
impl Drop for OriginGuard {
    fn drop(&mut self) {
        self.0.borrow_mut().origin = std::mem::take(&mut self.1);
    }
}

impl JsRuntime {
    /// Execute a CommonJS file, checkpoint reactions, then use run_event_loop for host work.
    pub fn execute_commonjs(&mut self, path: impl AsRef<Path>) -> Result<(), JsError> {
        let path = path.as_ref();
        let origin = path.to_string_lossy();
        let execution = (|| {
            let path = canonical_file(path, &origin)?;
            v8::scope!(let scope, &mut self.isolate);
            let context = v8::Local::new(scope, &self.context);
            let scope = &mut v8::ContextScope::new(scope, context);
            v8::tc_scope!(let scope, scope);
            load(scope, &path, true).ok_or_else(|| caught_error(scope, &origin))?;
            Ok(())
        })();
        let checkpoint = self.checkpoint();
        let result = execution.and(checkpoint);
        if result.is_err() {
            self.cancel_pending();
        }
        result
    }
}

fn throw_error(scope: &mut v8::PinScope, message: &str) {
    if let Some(message) = v8::String::new(scope, message) {
        let error = v8::Exception::error(scope, message);
        scope.throw_exception(error);
    }
}

fn host<T>(scope: &mut v8::PinScope, outcome: Result<T, JsError>) -> Option<T> {
    match outcome {
        Ok(value) => Some(value),
        Err(error) => {
            throw_error(scope, &error.to_string());
            None
        }
    }
}

fn put<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
    value: v8::Local<'s, v8::Value>,
) -> Option<()> {
    let key = v8::String::new(scope, name)?;
    match object.create_data_property(scope, key.into(), value) {
        Some(true) => Some(()),
        Some(false) => {
            throw_error(scope, "could not update CommonJS module metadata");
            None
        }
        None => None,
    }
}

fn exports<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    module: v8::Local<'s, v8::Object>,
) -> Option<v8::Local<'s, v8::Value>> {
    let key = v8::String::new(scope, "exports")?;
    module.get(scope, key.into())
}

fn parse_json<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    text: v8::Local<'s, v8::String>,
    path: &Path,
) -> Option<v8::Local<'s, v8::Value>> {
    v8::tc_scope!(let scope, scope);
    if let Some(value) = v8::json::parse(scope, text) {
        return Some(value);
    }
    if let Some(exception) = scope.exception() {
        let message = v8::Exception::create_message(scope, exception)
            .get(scope)
            .to_rust_string_lossy(scope);
        if let Ok(object) = v8::Local::<v8::Object>::try_from(exception)
            && let Some(message) = v8::String::new(scope, &format!("{}: {message}", path.display()))
        {
            put(scope, object, "message", message.into())?;
        }
        scope.rethrow();
    }
    None
}

fn resolve(specifier: &str, referrer: &str) -> Result<ModuleId, JsError> {
    if specifier.starts_with("rustyjs:") {
        return Builtin::resolve(specifier)
            .map(ModuleId::Builtin)
            .ok_or_else(|| {
                JsError::host(&format!("unknown built-in module {specifier:?}"), referrer)
            });
    }
    let path = Path::new(specifier);
    if path.is_absolute() || specifier.starts_with("./") || specifier.starts_with("../") {
        let base = Path::new(referrer)
            .parent()
            .ok_or_else(|| JsError::host("require has no file base", referrer))?;
        return packages::legacy_require(&normalize_absolute(&base.join(path)), referrer)
            .map(ModuleId::File);
    }
    packages::resolve_mode(specifier, referrer, LoadMode::Require)
}

fn require(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Some((specifier, referrer)) = require_arguments(scope, &args) else {
        return;
    };
    let outcome = resolve(&specifier, &referrer);
    let Some(target) = host(scope, outcome) else {
        return;
    };
    let value = match target {
        ModuleId::File(path) => load(scope, &path, false),
        ModuleId::Builtin(builtin) => load_builtin(scope, builtin).map(Into::into),
    };
    if let Some(value) = value {
        result.set(value);
    }
}

fn require_resolve(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Some((specifier, referrer)) = require_arguments(scope, &args) else {
        return;
    };
    let outcome = resolve(&specifier, &referrer);
    let Some(target) = host(scope, outcome) else {
        return;
    };
    let filename = match target {
        ModuleId::File(path) => path.to_string_lossy().into_owned(),
        ModuleId::Builtin(builtin) => builtin.specifier().into(),
    };
    if let Some(value) = v8::String::new(scope, &filename) {
        result.set(value.into());
    }
}

fn require_arguments(
    scope: &mut v8::PinScope,
    args: &v8::FunctionCallbackArguments,
) -> Option<(String, String)> {
    let Ok(specifier) = v8::Local::<v8::String>::try_from(args.get(0)) else {
        let message = v8::String::new(scope, "require expects a string specifier")?;
        let error = v8::Exception::type_error(scope, message);
        scope.throw_exception(error);
        return None;
    };
    Some((
        specifier.to_rust_string_lossy(scope),
        args.data().to_rust_string_lossy(scope),
    ))
}

fn load_builtin<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    builtin: Builtin,
) -> Option<v8::Local<'s, v8::Object>> {
    let state = scope.get_slot::<SharedCommonJs>()?.clone();
    let cached = {
        state
            .borrow()
            .builtins
            .get(&builtin)
            .map(|value| v8::Local::new(scope, value))
    };
    if let Some(cached) = cached {
        return Some(cached);
    }
    let object = builtins::commonjs_object(scope, builtin)?;
    state
        .borrow_mut()
        .builtins
        .insert(builtin, v8::Global::new(scope, object));
    Some(object)
}

fn bound_require<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    filename: v8::Local<'s, v8::String>,
    state: &SharedCommonJs,
) -> Option<v8::Local<'s, v8::Function>> {
    let function = v8::Function::builder(require)
        .data(filename.into())
        .build(scope)?;
    let resolver = v8::Function::builder(require_resolve)
        .data(filename.into())
        .build(scope)?;
    put(scope, function.into(), "resolve", resolver.into())?;
    let main = {
        state
            .borrow()
            .main
            .as_ref()
            .map(|main| v8::Local::new(scope, main))
    };
    let main = main
        .map(Into::into)
        .unwrap_or_else(|| v8::undefined(scope).into());
    put(scope, function.into(), "main", main)?;
    Some(function)
}

/// Normalize an explicit absolute path without consulting the process cwd.
fn normalize_absolute(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            part => normalized.push(part.as_os_str()),
        }
    }
    if path
        .as_os_str()
        .to_string_lossy()
        .chars()
        .last()
        .is_some_and(std::path::is_separator)
    {
        normalized.push("");
    }
    normalized
}

/// A filename anchor need not be a file that has been loaded or exists.
fn require_base(input: &str) -> Result<PathBuf, String> {
    let (path, directory) = if Path::new(input).is_absolute() {
        (
            PathBuf::from(input),
            input.chars().last().is_some_and(std::path::is_separator),
        )
    } else {
        let url = Url::parse(input)
            .map_err(|_| "createRequire expects an absolute filename or file URL string")?;
        if url.scheme() != "file" || url.query().is_some() || url.fragment().is_some() {
            return Err(
                "createRequire file URLs cannot have a query, fragment, or another scheme".into(),
            );
        }
        let encoded = url.path().to_ascii_lowercase();
        if encoded.contains("%2f") || encoded.contains("%5c") {
            return Err("createRequire rejects encoded path separators".into());
        }
        let directory = url.path().ends_with('/');
        (
            url.to_file_path()
                .map_err(|_| "createRequire file URL is not a local path")?,
            directory,
        )
    };
    if path.as_os_str().to_string_lossy().contains('\0') {
        return Err("createRequire rejects NUL paths".into());
    }
    let mut normalized = normalize_absolute(&path);
    if directory || normalized.file_name().is_none() {
        normalized.push("__rustyjs_require__.cjs");
    }
    let mut ancestor = normalized.as_path();
    let mut missing = Vec::new();
    loop {
        match std::fs::canonicalize(ancestor) {
            Ok(mut resolved) => {
                if !missing.is_empty() && !resolved.is_dir() {
                    return Err("createRequire base parent is not a directory".into());
                }
                for part in missing.into_iter().rev() {
                    resolved.push(part);
                }
                if resolved.to_str().is_none() {
                    return Err("createRequire base must be representable as UTF-8".into());
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                missing.push(
                    ancestor
                        .file_name()
                        .ok_or("createRequire base has no filename")?
                        .to_os_string(),
                );
                ancestor = ancestor
                    .parent()
                    .ok_or("createRequire base has no absolute ancestor")?;
            }
            Err(error) => {
                return Err(format!(
                    "cannot anchor createRequire at {}: {error}",
                    ancestor.display()
                ));
            }
        }
    }
}

pub(crate) fn create_require(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let base = v8::Local::<v8::String>::try_from(args.get(0))
        .ok()
        .map(|input| input.to_rust_string_lossy(scope))
        .ok_or_else(|| "createRequire expects an absolute filename or file URL string".to_owned())
        .and_then(|input| require_base(&input));
    let path = match base {
        Ok(path) => path,
        Err(message) => {
            if let Some(message) = v8::String::new(scope, &message) {
                let error = v8::Exception::type_error(scope, message);
                scope.throw_exception(error);
            }
            return;
        }
    };
    let Some(filename) = v8::String::new(scope, &path.to_string_lossy()) else {
        return;
    };
    let Some(state) = scope.get_slot::<SharedCommonJs>().cloned() else {
        return;
    };
    if let Some(require) = bound_require(scope, filename, &state) {
        result.set(require.into());
    }
}

pub(crate) fn load<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    path: &Path,
    entry: bool,
) -> Option<v8::Local<'s, v8::Value>> {
    let state = scope.get_slot::<SharedCommonJs>()?.clone();
    if state.borrow().depth >= 64 {
        throw_error(scope, "CommonJS require depth exceeds 64");
        return None;
    }
    state.borrow_mut().depth += 1;
    let _depth = DepthGuard(state.clone());
    let host_state = scope.get_slot::<SharedState>()?.clone();
    let previous = std::mem::replace(
        &mut host_state.borrow_mut().origin,
        path.to_string_lossy().into_owned(),
    );
    let _origin = OriginGuard(host_state, previous);
    let cached = {
        state
            .borrow()
            .cache
            .get(path)
            .map(|module| v8::Local::new(scope, module))
    };
    if let Some(module) = cached {
        return exports(scope, module);
    }
    let origin = path.to_string_lossy();
    let kind = host(scope, packages::format(path, &origin))?;
    let esm_cached = scope
        .get_slot::<SharedModules>()?
        .borrow()
        .is_source_module(scope, path);
    if kind == Format::Esm || esm_cached {
        throw_error(
            scope,
            "require() of ES modules is not supported; use import()",
        );
        return None;
    }
    let module = v8::Object::new(scope);
    let initial_exports = v8::Object::new(scope);
    put(scope, module, "exports", initial_exports.into())?;
    let filename = v8::String::new(scope, &origin)?;
    let is_main = entry && state.borrow().main.is_none();
    let id = if is_main {
        v8::String::new(scope, ".")?
    } else {
        filename
    };
    put(scope, module, "id", id.into())?;
    put(scope, module, "filename", filename.into())?;
    let loaded = v8::Boolean::new(scope, false);
    put(scope, module, "loaded", loaded.into())?;
    state
        .borrow_mut()
        .cache
        .insert(path.to_path_buf(), v8::Global::new(scope, module));
    if is_main {
        state.borrow_mut().main = Some(v8::Global::new(scope, module));
    }
    let mut cleanup = LoadGuard {
        state: state.clone(),
        path: path.to_path_buf(),
        main: is_main,
        success: false,
    };
    let require = bound_require(scope, filename, &state)?;
    put(scope, module, "require", require.into())?;
    let source = host(
        scope,
        std::fs::read_to_string(path).map_err(|error| {
            JsError::host(&format!("cannot read CommonJS module: {error}"), &origin)
        }),
    )?;
    let source = source.strip_prefix('\u{feff}').unwrap_or(&source);
    if kind == Format::Json {
        let text = v8::String::new(scope, source)?;
        let value = parse_json(scope, text, path)?;
        put(scope, module, "exports", value)?;
    } else {
        let source = if source.starts_with("#!") {
            let (header, tail) = source.split_once('\n').unwrap_or((source, ""));
            format!("{}\n{tail}", " ".repeat(header.len()))
        } else {
            source.to_owned()
        };
        let source = v8::String::new(scope, &source)?;
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
        let mut source = v8::script_compiler::Source::new(source, Some(&script_origin));
        let names = ["exports", "require", "module", "__filename", "__dirname"]
            .into_iter()
            .map(|name| v8::String::new(scope, name))
            .collect::<Option<Vec<_>>>()?;
        let function = v8::script_compiler::compile_function(
            scope,
            &mut source,
            &names,
            &[],
            v8::script_compiler::CompileOptions::NoCompileOptions,
            v8::script_compiler::NoCacheReason::NoReason,
        )?;
        let directory = v8::String::new(scope, &path.parent()?.to_string_lossy())?;
        function.call(
            scope,
            initial_exports.into(),
            &[
                initial_exports.into(),
                require.into(),
                module.into(),
                filename.into(),
                directory.into(),
            ],
        )?;
    }
    let loaded = v8::Boolean::new(scope, true);
    put(scope, module, "loaded", loaded.into())?;
    let value = exports(scope, module)?;
    cleanup.success = true;
    Some(value)
}

pub(crate) fn create_bridge<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    path: &Path,
) -> Option<v8::Local<'s, v8::Module>> {
    let name = v8::String::new(scope, &path.to_string_lossy())?;
    let export = v8::String::new(scope, "default")?;
    Some(v8::Module::create_synthetic_module(
        scope,
        name,
        &[export],
        evaluate_bridge,
    ))
}

fn evaluate_bridge<'s>(
    context: v8::Local<'s, v8::Context>,
    module: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Value>> {
    // SAFETY: V8 invokes synthetic evaluation in the live context on its owner thread.
    v8::callback_scope!(unsafe scope, context);
    let modules = scope.get_slot::<SharedModules>()?.clone();
    let path = modules.borrow().file_for_module(module)?;
    let value = load(scope, &path, false)?;
    let name = v8::String::new(scope, "default")?;
    if module.set_synthetic_module_export(scope, name, value) != Some(true) {
        return None;
    }
    let resolver = v8::PromiseResolver::new(scope)?;
    let value = v8::undefined(scope);
    resolver.resolve(scope, value.into())?;
    Some(resolver.get_promise(scope).into())
}
