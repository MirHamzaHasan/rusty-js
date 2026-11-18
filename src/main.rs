use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "RustyJS — JavaScript on V8 with a Rust host\n\nUsage:\n  rustyjs <file.js|file.mjs>\n  rustyjs --module <file>\n  rustyjs -e <source>\n  rustyjs --help\n  rustyjs --version\n\n.mjs files use ES modules. Use --module for an ES module with another extension.\nUse -- before a filename beginning with '-'.\n";

enum Command {
    Help,
    Version,
    Eval(String),
    File(PathBuf),
    Module(PathBuf),
}

enum Input {
    Script(String, String),
    Module(PathBuf),
}

fn parse_args() -> Result<Command, String> {
    let mut args = std::env::args_os().skip(1);
    let first = args.next().ok_or_else(|| HELP.to_owned())?;
    let command = match first.to_str() {
        Some("-h" | "--help") => Command::Help,
        Some("-v" | "--version") => Command::Version,
        Some("-m" | "--module") => Command::Module(PathBuf::from(
            args.next().ok_or("--module requires a filename")?,
        )),
        Some("-e" | "--eval") => {
            let source = args.next().ok_or("--eval requires JavaScript source")?;
            Command::Eval(
                source
                    .into_string()
                    .map_err(|_| "source must be valid UTF-8")?,
            )
        }
        Some("--") => Command::File(PathBuf::from(
            args.next().ok_or("expected a filename after --")?,
        )),
        Some(option) if option.starts_with('-') => return Err(format!("unknown option: {option}")),
        _ => Command::File(PathBuf::from(first)),
    };
    if let Some(extra) = args.next() {
        return Err(format!("unexpected argument: {}", display_arg(extra)));
    }
    Ok(command)
}

fn display_arg(arg: OsString) -> String {
    arg.to_string_lossy().into_owned()
}

fn run(command: Command) -> Result<(), String> {
    let input = match command {
        Command::Help => return write_stdout(HELP),
        Command::Version => {
            return write_stdout(concat!("rustyjs ", env!("CARGO_PKG_VERSION"), "\n"));
        }
        Command::Eval(source) => Input::Script(source, "<eval>".into()),
        Command::Module(path) => Input::Module(path),
        Command::File(path) if path.extension().is_some_and(|extension| extension == "mjs") => {
            Input::Module(path)
        }
        Command::File(path) => {
            let source = std::fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            Input::Script(source, path.to_string_lossy().into_owned())
        }
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|error| format!("could not start Tokio: {error}"))?;
    runtime.block_on(async {
        let mut js = rustyjs::JsRuntime::new().map_err(|error| error.to_string())?;
        match input {
            Input::Script(source, origin) => js.execute(&source, &origin),
            Input::Module(path) => js.execute_module(path),
        }
        .map_err(|error| error.to_string())?;
        js.run_event_loop().await.map_err(|error| error.to_string())
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
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "{error}");
            ExitCode::FAILURE
        }
    }
}
