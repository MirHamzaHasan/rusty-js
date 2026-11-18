//! Buffer owns V8 byte storage. UTF-8 conversion uses safe, scope-local copies.

const MAX_LENGTH: usize = 64 * 1024 * 1024;

pub(crate) fn create<'s>(scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, v8::Value>> {
    let code = v8::String::new(scope, include_str!("js/buffer.js"))?;
    let name = v8::String::new(scope, "rustyjs:buffer")?;
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
    let parameters = ["encodeUtf8", "decodeUtf8", "utf8Length", "maxLength"]
        .map(|name| v8::String::new(scope, name))
        .into_iter()
        .collect::<Option<Vec<_>>>()?;
    let function = v8::script_compiler::compile_function(
        scope,
        &mut source,
        &parameters,
        &[],
        v8::script_compiler::CompileOptions::NoCompileOptions,
        v8::script_compiler::NoCacheReason::NoReason,
    )?;
    let encode = v8::Function::new(scope, encode_utf8)?;
    let decode = v8::Function::new(scope, decode_utf8)?;
    let length = v8::Function::new(scope, utf8_length)?;
    let max = v8::Number::new(scope, MAX_LENGTH as f64);
    let receiver = v8::undefined(scope);
    function.call(
        scope,
        receiver.into(),
        &[encode.into(), decode.into(), length.into(), max.into()],
    )
}

fn range_error(scope: &mut v8::PinScope) {
    if let Some(message) = v8::String::new(scope, "Buffer exceeds the 64 MiB allocation limit") {
        let error = v8::Exception::range_error(scope, message);
        scope.throw_exception(error);
    }
}

fn encode_utf8(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Ok(string) = v8::Local::<v8::String>::try_from(args.get(0)) else {
        return;
    };
    let length = string.utf8_length(scope);
    if length > MAX_LENGTH {
        range_error(scope);
        return;
    }
    let mut bytes = vec![0; length];
    let written =
        string.write_utf8_v2(scope, &mut bytes, v8::WriteFlags::kReplaceInvalidUtf8, None);
    bytes.truncate(written);
    let store = v8::ArrayBuffer::new_backing_store_from_bytes(bytes).make_shared();
    let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
    if let Some(view) = v8::Uint8Array::new(scope, buffer, 0, written) {
        result.set(view.into());
    }
}

fn decode_utf8(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Ok(view) = v8::Local::<v8::Uint8Array>::try_from(args.get(0)) else {
        return;
    };
    let length = view.byte_length();
    if length > MAX_LENGTH {
        range_error(scope);
        return;
    }
    let mut bytes = vec![0; length];
    let copied = view.copy_contents(&mut bytes);
    bytes.truncate(copied);
    if let Some(string) = v8::String::new(scope, &String::from_utf8_lossy(&bytes)) {
        result.set(string.into());
    }
}

fn utf8_length(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    if let Ok(string) = v8::Local::<v8::String>::try_from(args.get(0)) {
        result.set(v8::Number::new(scope, string.utf8_length(scope) as f64).into());
    }
}
