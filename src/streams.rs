//! Stream state and callbacks live in V8 and use existing Promise checkpoints.

pub(crate) struct Exports<'s> {
    pub stream: [v8::Local<'s, v8::Value>; 5],
    pub promises: [v8::Local<'s, v8::Value>; 2],
}

pub(crate) fn create<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    events: v8::Local<'s, v8::Value>,
    buffer: v8::Local<'s, v8::Value>,
) -> Option<Exports<'s>> {
    let lifecycle = compile(scope, include_str!("js/stream_lifecycle.js"), &[], &[])?;
    let parameters = ["EventEmitter", "Buffer", "Lifecycle"];
    let arguments = [events, buffer, lifecycle];
    let writable = compile(
        scope,
        include_str!("js/writable.js"),
        &parameters,
        &arguments,
    )?;
    let readable = compile(
        scope,
        include_str!("js/readable.js"),
        &parameters,
        &arguments,
    )?;
    let helpers = compile(
        scope,
        include_str!("js/stream_helpers.js"),
        &["EventEmitter", "Readable", "Writable", "Lifecycle"],
        &[events, readable, writable, lifecycle],
    )?;
    let helpers = v8::Local::<v8::Object>::try_from(helpers).ok()?;
    let finished = property(scope, helpers, "finished")?;
    let pipeline = property(scope, helpers, "pipeline")?;
    let promises = property(scope, helpers, "promises")?;
    let object = v8::Local::<v8::Object>::try_from(promises).ok()?;
    Some(Exports {
        stream: [writable, readable, finished, pipeline, promises],
        promises: [
            property(scope, object, "finished")?,
            property(scope, object, "pipeline")?,
        ],
    })
}

fn property<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
) -> Option<v8::Local<'s, v8::Value>> {
    let key = v8::String::new(scope, name)?;
    object.get(scope, key.into())
}

fn compile<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    source: &str,
    parameters: &[&str],
    arguments: &[v8::Local<'s, v8::Value>],
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
    let parameters = parameters
        .iter()
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
    let receiver = v8::undefined(scope);
    function.call(scope, receiver.into(), arguments)
}
