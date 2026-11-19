//! Runtime-local process configuration and synchronous host facts.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Instant;

/// Explicit guest command line/environment. Defaults inherit neither from the host.
#[derive(Clone, Default)]
pub struct RuntimeOptions {
    /// Values following the executable in process.argv (include the entry filename if desired).
    pub args: Vec<String>,
    pub exec_args: Vec<String>,
    pub argv0: Option<String>,
    pub env: BTreeMap<String, String>,
}

pub(crate) struct HostState {
    started: Instant,
    pub exit_code: Cell<Option<u8>>,
    executable: String,
    options: RuntimeOptions,
}
pub(crate) type SharedHost = Rc<HostState>;

impl HostState {
    pub fn new(options: RuntimeOptions) -> Result<Self, String> {
        if options
            .env
            .keys()
            .any(|key| key.is_empty() || key.contains(['\0', '=']))
        {
            return Err("invalid environment variable name".into());
        }
        let executable = std::env::current_exe()
            .map_err(|error| format!("cannot read executable path: {error}"))?
            .into_os_string()
            .into_string()
            .map_err(|_| "executable path is not valid UTF-8")?;
        Ok(Self {
            started: Instant::now(),
            exit_code: Cell::new(None),
            executable,
            options,
        })
    }
}

fn platform() -> &'static str {
    match std::env::consts::OS {
        "macos" | "ios" => "darwin",
        "windows" => "win32",
        other => other,
    }
}
fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "ia32",
        "powerpc64" => "ppc64",
        other => other,
    }
}
fn os_type() -> &'static str {
    match std::env::consts::OS {
        "macos" | "ios" => "Darwin",
        "windows" => "Windows_NT",
        "linux" | "android" => "Linux",
        "freebsd" => "FreeBSD",
        "openbsd" => "OpenBSD",
        "netbsd" => "NetBSD",
        "dragonfly" => "DragonFly",
        other => other,
    }
}

pub(crate) fn create<'s>(scope: &mut v8::PinScope<'s, '_>) -> Option<v8::Local<'s, v8::Object>> {
    let host = scope.get_slot::<SharedHost>()?.clone();
    let mut argv = vec![host.executable.clone()];
    argv.extend(host.options.args.iter().cloned());
    let data = serde_json::json!({
        "argv": argv, "execArgv": host.options.exec_args, "argv0": host.options.argv0.as_ref().unwrap_or(&host.executable),
        "execPath": host.executable, "env": host.options.env, "platform": platform(), "arch": arch(),
        "pid": std::process::id(), "version": concat!("v",env!("CARGO_PKG_VERSION")),
        "versions": {"rustyjs": env!("CARGO_PKG_VERSION"), "v8": v8::V8::get_version()},
        "osType": os_type(), "endianness": if cfg!(target_endian="little") {"LE"} else {"BE"},
        "EOL": if cfg!(windows) {"\r\n"} else {"\n"}, "devNull": if cfg!(windows) {"\\\\.\\nul"} else {"/dev/null"},
    });
    let json = v8::String::new(scope, &serde_json::to_string(&data).ok()?)?;
    let info = v8::json::parse(scope, json)?;
    let code = v8::String::new(scope, include_str!("js/host.js"))?;
    let name = v8::String::new(scope, "rustyjs:process")?;
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
    let parameters = [
        "info",
        "cwd",
        "parallelism",
        "home",
        "temp",
        "elapsed",
        "status",
    ]
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
    let cwd = v8::Function::new(scope, crate::paths::cwd)?;
    let parallelism = v8::Function::new(scope, available_parallelism)?;
    let home = v8::Function::new(scope, home_dir)?;
    let temp = v8::Function::new(scope, temp_dir)?;
    let elapsed = v8::Function::new(scope, elapsed_ns)?;
    let status = v8::Function::new(scope, exit_status)?;
    let receiver = v8::undefined(scope);
    v8::Local::<v8::Object>::try_from(function.call(
        scope,
        receiver.into(),
        &[
            info,
            cwd.into(),
            parallelism.into(),
            home.into(),
            temp.into(),
            elapsed.into(),
            status.into(),
        ],
    )?)
    .ok()
}

fn throw(scope: &mut v8::PinScope, message: &str) {
    if let Some(message) = v8::String::new(scope, message) {
        let error = v8::Exception::error(scope, message);
        scope.throw_exception(error);
    }
}
fn available_parallelism(
    scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    match std::thread::available_parallelism() {
        Ok(count) => result.set(v8::Number::new(scope, count.get() as f64).into()),
        Err(error) => throw(
            scope,
            &format!("cannot read available parallelism: {error}"),
        ),
    }
}
fn return_path(scope: &mut v8::PinScope, path: &std::path::Path, mut result: v8::ReturnValue) {
    let Some(value) = path.to_str() else {
        throw(scope, "host path is not valid UTF-8");
        return;
    };
    if let Some(value) = v8::String::new(scope, value) {
        result.set(value.into());
    }
}
fn home_dir(
    scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    result: v8::ReturnValue,
) {
    match std::env::home_dir() {
        Some(path) => return_path(scope, &path, result),
        None => throw(scope, "cannot determine home directory"),
    }
}
fn temp_dir(
    scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    result: v8::ReturnValue,
) {
    return_path(scope, &std::env::temp_dir(), result);
}
fn elapsed_ns(
    scope: &mut v8::PinScope,
    _args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Some(host) = scope.get_slot::<SharedHost>().cloned() else {
        return;
    };
    match u64::try_from(host.started.elapsed().as_nanos()) {
        Ok(value) => result.set(v8::BigInt::new_from_u64(scope, value).into()),
        Err(_) => throw(
            scope,
            "runtime clock exceeded its supported nanosecond range",
        ),
    }
}
fn exit_status(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut result: v8::ReturnValue,
) {
    let Some(host) = scope.get_slot::<SharedHost>().cloned() else {
        return;
    };
    if args.length() == 0 {
        if let Some(code) = host.exit_code.get() {
            result.set(v8::Integer::new(scope, code.into()).into());
        }
        return;
    }
    let value = args.get(0);
    if value.is_undefined() {
        host.exit_code.set(None);
        return;
    }
    let number = if value.is_number() {
        value.number_value(scope)
    } else {
        None
    };
    let Some(number) = number else {
        if let Some(message) = v8::String::new(scope, "exitCode must be a number or undefined") {
            let error = v8::Exception::type_error(scope, message);
            scope.throw_exception(error);
        }
        return;
    };
    if !number.is_finite() || number.fract() != 0.0 || !(0.0..=255.0).contains(&number) {
        if let Some(message) = v8::String::new(scope, "exitCode must be an integer in 0..255") {
            let error = v8::Exception::range_error(scope, message);
            scope.throw_exception(error);
        }
        return;
    }
    host.exit_code.set(Some(number as u8));
}
