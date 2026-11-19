//! Lexical path operations run in V8; only cwd crosses into the host.

pub(crate) fn create<'s>(scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, v8::Object>> {
    let code = v8::String::new(scope, include_str!("js/path.js"))?;
    let name = v8::String::new(scope, "rustyjs:path")?;
    let origin = v8::ScriptOrigin::new(
        scope,
        name.into(),
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
    let mut source = v8::script_compiler::Source::new(code, Some(&origin));
    let parameters = ["cwd", "isWindows"]
        .into_iter()
        .map(|name| v8::String::new(scope, name))
        .collect::<Option<Vec<_>>>()?;
    let function = v8::script_compiler::compile_function(
        scope,
        &mut source,
        &parameters,
        &[],
        v8::script_compiler::CompileOptions::NoCompileOptions,
        v8::script_compiler::NoCacheReason::NoReason,
    )?;
    let cwd = v8::Function::new(scope, cwd)?;
    let windows = v8::Boolean::new(scope, cfg!(windows));
    let receiver = v8::undefined(scope);
    v8::Local::<v8::Object>::try_from(function.call(
        scope,
        receiver.into(),
        &[cwd.into(), windows.into()],
    )?)
    .ok()
}

pub(crate) fn cwd(
    scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let path = match std::env::current_dir() {
        Ok(path) => path,
        Err(error) => {
            throw(scope, &format!("cannot read current directory: {error}"));
            return;
        }
    };
    let Some(path) = path.to_str() else {
        throw(scope, "current directory is not valid UTF-8");
        return;
    };
    if let Some(path) = v8::String::new(scope, path) {
        result.set(path.into());
    }
}

fn throw(scope: &mut v8::PinScope, message: &str) {
    if let Some(message) = v8::String::new(scope, message) {
        let error = v8::Exception::error(scope, message);
        scope.throw_exception(error);
    }
}
