//! EventEmitter runs in V8; only warning output crosses into the Rust host.

use std::io::{self, Write};

pub(crate) fn create<'s>(scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, v8::Value>> {
    let code = v8::String::new(scope, include_str!("js/events.js"))?;
    let name = v8::String::new(scope, "rustyjs:events")?;
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
    let parameter = v8::String::new(scope, "warn")?;
    let function = v8::script_compiler::compile_function(
        scope,
        &mut source,
        &[parameter],
        &[],
        v8::script_compiler::CompileOptions::NoCompileOptions,
        v8::script_compiler::NoCacheReason::NoReason,
    )?;
    let warn = v8::Function::new(scope, warning)?;
    let receiver = v8::undefined(scope);
    function.call(scope, receiver.into(), &[warn.into()])
}

fn warning(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _result: v8::ReturnValue,
) {
    let Ok(message) = v8::Local::<v8::String>::try_from(args.get(0)) else {
        return;
    };
    let message = message.to_rust_string_lossy(scope);
    let outcome = writeln!(io::stderr().lock(), "{message}");
    if let Err(error) = outcome
        && let Some(message) = v8::String::new(
            scope,
            &format!("cannot write EventEmitter warning: {error}"),
        )
    {
        let error = v8::Exception::error(scope, message);
        scope.throw_exception(error);
    }
}
