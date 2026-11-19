use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "RustyJS — JavaScript on V8 with a Rust host\n\nUsage:\n  rustyjs <file.js|file.mjs|file.cjs> [args...]\n  rustyjs --module <file> [args...]\n  rustyjs --commonjs <file> [args...]\n  rustyjs -e <source> [--] [args...]\n  rustyjs --help\n  rustyjs --version\n\n.mjs files use ES modules; .cjs files use CommonJS.\nUse --module or --commonjs to select a file mode explicitly.\nUse -- before a filename beginning with '-'.\nArguments after the entrypoint belong to the script.\n";

enum Command {
    Help,
    Version,
    Eval(String),
    File(PathBuf),
    Module(PathBuf),
    CommonJs(PathBuf),
}

enum Input {
    Script(String, String),
    Module(PathBuf),
    CommonJs(PathBuf),
}

struct Invocation {
    command: Command,
    args: Vec<String>,
    exec_args: Vec<String>,
    argv0: String,
}

fn parse_args() -> Result<Invocation, String> {
    let mut args = std::env::args_os();
    let argv0 = args
        .next()
        .ok_or("missing executable argument")?
        .into_string()
        .map_err(|_| "argv0 must be valid UTF-8")?;
    let first = args.next().ok_or_else(|| HELP.to_owned())?;
    let mut exec_args = Vec::new();
    let command = match first.to_str() {
        Some("-h" | "--help") => Command::Help,
        Some("-v" | "--version") => Command::Version,
        Some(option @ ("-m" | "--module")) => {
            exec_args.push(option.into());
            Command::Module(PathBuf::from(
                args.next().ok_or("--module requires a filename")?,
            ))
        }
        Some("--commonjs") => {
            exec_args.push("--commonjs".into());
            Command::CommonJs(PathBuf::from(
                args.next().ok_or("--commonjs requires a filename")?,
            ))
        }
        Some("-e" | "--eval") => {
            let source = args.next().ok_or("--eval requires JavaScript source")?;
            let source = source
                .into_string()
                .map_err(|_| "source must be valid UTF-8")?;
            exec_args.extend([first.to_string_lossy().into_owned(), source.clone()]);
            Command::Eval(source)
        }
        Some("--") => Command::File(PathBuf::from(
            args.next().ok_or("expected a filename after --")?,
        )),
        Some(option) if option.starts_with('-') => return Err(format!("unknown option: {option}")),
        _ => Command::File(PathBuf::from(first)),
    };
    let mut tail = args.collect::<Vec<_>>();
    if matches!(command, Command::Help | Command::Version)
        && let Some(extra) = tail.first()
    {
        return Err(format!("unexpected argument: {}", extra.to_string_lossy()));
    }
    if matches!(command, Command::Eval(_)) && tail.first().is_some_and(|arg| arg == "--") {
        tail.remove(0);
    }
    let args = tail
        .into_iter()
        .map(|arg| {
            arg.into_string()
                .map_err(|_| "script arguments must be valid UTF-8".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Invocation {
        command,
        args,
        exec_args,
        argv0,
    })
}

fn run(invocation: Invocation) -> Result<u8, String> {
    let Invocation {
        command,
        mut args,
        exec_args,
        argv0,
    } = invocation;
    if let Command::File(path) | Command::Module(path) | Command::CommonJs(path) = &command {
        let absolute = std::path::absolute(path)
            .map_err(|error| format!("cannot resolve entrypoint: {error}"))?;
        args.insert(
            0,
            absolute
                .into_os_string()
                .into_string()
                .map_err(|_| "entrypoint must be valid UTF-8")?,
        );
    }
    let input = match command {
        Command::Help => return write_stdout(HELP).map(|()| 0),
        Command::Version => {
            return write_stdout(concat!("rustyjs ", env!("CARGO_PKG_VERSION"), "\n")).map(|()| 0);
        }
        Command::Eval(source) => Input::Script(source, "<eval>".into()),
        Command::Module(path) => Input::Module(path),
        Command::CommonJs(path) => Input::CommonJs(path),
        Command::File(path) if path.extension().is_some_and(|extension| extension == "cjs") => {
            Input::CommonJs(path)
        }
        Command::File(path) if path.extension().is_some_and(|extension| extension == "mjs") => {
            Input::Module(path)
        }
        Command::File(path) => {
            let source = std::fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            Input::Script(source, path.to_string_lossy().into_owned())
        }
    };
    let options = rustyjs::RuntimeOptions {
        args,
        exec_args,
        argv0: Some(argv0),
        env: std::env::vars_os()
            .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
            .filter(|(key, _)| !key.is_empty() && !key.contains(['\0', '=']))
            .collect(),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|error| format!("could not start Tokio: {error}"))?;
    runtime.block_on(async {
        let mut js =
            rustyjs::JsRuntime::with_options(options).map_err(|error| error.to_string())?;
        match input {
            Input::Script(source, origin) => js.execute(&source, &origin),
            Input::Module(path) => js.execute_module(path),
            Input::CommonJs(path) => js.execute_commonjs(path),
        }
        .map_err(|error| error.to_string())?;
        js.run_event_loop()
            .await
            .map_err(|error| error.to_string())?;
        Ok(js.exit_code().unwrap_or(0))
    })
}

fn write_stdout(text: &str) -> Result<(), String> {
    io::stdout()
        .lock()
        .write_all(text.as_bytes())
        .map_err(|error| error.to_string())
}

fn main() -> ExitCode {
    match parse_args().and_then(run) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "{error}");
            ExitCode::FAILURE
        }
    }
}
