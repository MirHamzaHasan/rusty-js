# OS and process foundation

The runtime exposes a global process object and matching default/CommonJS exports
through process/node:process/rustyjs:process. os/node:os/rustyjs:os expose a default
OS utility object. Each alias set shares its namespace and underlying object.
Named exports capture initial values/functions independently of later object
property replacement. There is no full Node process compatibility implied.

## CLI metadata and status

`process.argv` starts with execPath. File execution adds the absolute supplied
entry filename, then script arguments. `std::path::absolute` supplies that filename;
this does not resolve symlinks or promise complete dot-segment normalization.
Inline eval has no synthetic filename argument. Runtime mode flags are separate
in execArgv; eval includes the flag and source. argv0 preserves the original
executable argument. Arguments after a file are passed literally, including `--`.
An initial `--` after inline source is consumed as its argument separator.

```sh
cargo run --locked -- examples/host/main.mjs one --flag
cargo run --locked -- -e 'console.log(process.argv.slice(1).join(","))' -- one --flag
```

The CLI inherits valid UTF-8 host environment entries. Non-UTF-8 arguments and
entrypoint metadata fail with a diagnostic. Non-UTF-8/invalid-name environment
entries are omitted, without printing their contents. Help/version reject extra
arguments; execution commands accept script arguments.

`process.exitCode` requests the eventual CLI status. It accepts an integer Number
in 0..255 or undefined to clear it. It neither exits the embedding process nor
cancels host work: the CLI drains the event loop before returning that status.
Uncaught execution errors still produce diagnostics and status 1. Replacing the
global process object does not replace the native exit-status cell. There is no
immediate process.exit/abort API or exit lifecycle event yet.

## Environment and embedding

The environment is a runtime-local null-prototype view. Writes coerce values to
strings; Symbols, empty/NUL/equals-containing names, accessors, nonmutable property
descriptors, prototype replacement, and freezing are rejected. Delete removes a
local entry. Coercion errors propagate as guest exceptions. Edits never mutate
the Rust process environment or another runtime's snapshot. Windows name lookup
is case-sensitive in this view rather than emulating all Windows environment rules.

The library's `JsRuntime::new()` inherits neither host arguments nor environment.
Its argv contains only execPath. Configure guest metadata with
`JsRuntime::with_options(RuntimeOptions { ... })`. The public fields are args
(values following the executable), exec_args, argv0, and env (a string BTreeMap).
Invalid environment names return a host error without including values.
`JsRuntime::exit_code()` reads the guest's requested status without executing JS.

```rust
let mut options = rustyjs::RuntimeOptions::default();
options.args = vec!["embedded.js".into(), "one".into()];
options.env.insert("MODE".into(), "test".into());
let mut js = rustyjs::JsRuntime::with_options(options)?;
js.execute("process.exitCode = 4", "embedded.js")?;
assert_eq!(js.exit_code(), Some(4));
```

These configuration defaults are not a sandbox. Native cwd/home/temp and other
host facts remain accessible to trusted scripts. OS helpers read the real host
environment/account information, so editing the JS environment view does not
change homedir/tmpdir results.

## Supported fields and clocks

Process named exports are default, argv, execArgv, argv0, execPath, platform, arch,
pid, version, versions, release, env, cwd, uptime, and hrtime. exitCode is accessed
on the default/global object, not as a named live binding. argv/execArgv and env
are mutable; named bindings retain their initial object references after property
replacement. Identity labels are read-only; versions/release are frozen.

version is RustyJS's version with a v prefix. versions contains rustyjs and the
pinned V8 engine version; it does not advertise a Node version or ABI. release.name
is rustyjs. platform/arch use Node-style spellings such as darwin/win32 and
arm64/x64. pid is the real host PID. cwd shares the existing controlled host-path
error contract. No chdir, stdio streams, nextTick, process events/signals, or
memory/CPU usage reporting is provided in this slice.

uptime and hrtime measure monotonic elapsed time since this runtime's host state
was created, rather than the embedding application's process start. hrtime()
returns [seconds, nanoseconds]; hrtime(previous) subtracts a validated two-integer
tuple and normalizes negative differences. hrtime.bigint() returns nanoseconds.
The private native clock uses a checked u64 nanosecond range and errors rather
than wrapping if that range is exceeded. It is not an execution timeout.

## OS utilities and implementation

OS named/default methods are platform, arch, type, endianness,
availableParallelism, homedir, and tmpdir, with EOL/devNull constants. Platform,
architecture, type, and endian labels describe the compilation target.
availableParallelism uses Rust's estimate and reports host query errors rather
than assuming a CPU count. homedir uses `std::env::home_dir` including its host
fallback; tmpdir uses Rust's host temp path and trims ordinary trailing separators.
Unrepresentable paths produce JS errors rather than lossy replacement.
Detailed CPU/network/memory facts, kernel release/version, os.uptime, and priorities
remain unsupported. Windows/Linux native behavior has not been validated here.

1. Runtime construction stores owned configuration, an Instant, and a Cell for
   exit status in an isolate-local Rc slot. It contains no persistent V8 handles.
2. JSON serialization supplies metadata to a bundled factory without evaluating
   it as source. Private native functions supply cwd, path facts, parallelism,
   elapsed time, and status. Guest conversion runs without a held registry borrow.
3. Built-in setup captures OS/process objects and named values before guest code;
   the global process references the same original object. Native callbacks stay
   on the isolate thread and never carry handles across await or to workers.
4. Cancellation preserves process context for reuse. Teardown removes the host
   slot alongside the existing module/operation slots before disposing V8.

Selected signatures reference the [Node process API](https://nodejs.org/download/release/v24.11.0/docs/api/process.html),
[Node OS API](https://nodejs.org/download/release/v24.11.0/docs/api/os.html), and
[Rust home directory contract](https://doc.rust-lang.org/std/env/fn.home_dir.html).
The differences above are explicit; no Node/package compatibility or speedup is claimed.

Eleven local host tests cover aliases, immutable identity, clocks, native facts,
CLI modes/arguments, environment isolation/conversion, library configuration and
reuse, eventual exit status, replacement resistance, and encoding policy. They run
on macOS arm64 with the full local suite. GitHub Actions remains idle.
