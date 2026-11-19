//! Stream state and callbacks live in V8 and use existing Promise checkpoints.

pub(crate) fn create<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    events: v8::Local<'s, v8::Value>,
    buffer: v8::Local<'s, v8::Value>,
) -> Option<[v8::Local<'s, v8::Value>; 2]> {
    let writable = compile(scope, include_str!("js/writable.js"), events, buffer)?;
    let readable = compile(scope, include_str!("js/readable.js"), events, buffer)?;
    Some([writable, readable])
}

fn compile<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    source: &str,
    events: v8::Local<'s, v8::Value>,
    buffer: v8::Local<'s, v8::Value>,
) -> Option<v8::Local<'s, v8::Value>> {
    let code = v8::String::new(scope, source)?;
    let name = v8::String::new(scope, "rustyjs:stream")?;
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
    let event_name = v8::String::new(scope, "EventEmitter")?;
    let buffer_name = v8::String::new(scope, "Buffer")?;
    let function = v8::script_compiler::compile_function(
        scope,
        &mut source,
        &[event_name, buffer_name],
        &[],
        v8::script_compiler::CompileOptions::NoCompileOptions,
        v8::script_compiler::NoCacheReason::NoReason,
    )?;
    let receiver = v8::undefined(scope);
    function.call(scope, receiver.into(), &[events, buffer])
}
