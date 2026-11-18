# RustyJS

A JavaScript runtime being built in Rust, embedding V8 through rusty_v8 with a
current-thread Tokio runtime for asynchronous host work.

The intended focus is native Rust facilities accessible from JavaScript: AI
sandboxes, blockchain primitives, dataframe and statistical operations, and GPU
compute. These domains are on the roadmap; this repository does not yet provide
their implementations. Raw JavaScript runs on V8. No performance results have been
established for RustyJS.

## Status

Phase 1 is underway. Core execution supports classic scripts from files or inline
source, native `console.log`, and JavaScript exception diagnostics with file origins
and available stack traces. It explicitly drains Promise microtasks and exposes
`Rusty.sleep(ms)` through a Tokio-to-JavaScript Promise bridge. The library keeps
its context usable after a caught guest error. The CLI runs until host work is idle.

Callback timers (`setTimeout`/`setInterval` and their clear functions) run on Tokio
deadlines with callbacks on the isolate thread. Local ES modules support static
and dynamic imports, file URLs, caching, cycles, host-backed top-level await, and
`import.meta.url` / `import.meta.main` metadata.
The first built-in module, `rustyjs:timers`, exports the native sleep and callback
timer functions through ESM.
Unhandled rejections fail the current run after its microtask checkpoint. This is
an early runtime for trusted scripts; it does not enforce time or memory limits.

## Run

Install stable Rust and your platform's C/C++ build tools. Cargo downloads V8's
prebuilt archive on the first build, which can take substantially longer than an
ordinary Rust crate. Dependencies are recorded in `Cargo.lock`.

```sh
cargo run --locked -- examples/hello.js
cargo run --locked -- examples/async.js
cargo run --locked -- examples/timers.js
cargo run --locked -- examples/modules/main.mjs
cargo run --locked -- examples/modules/dynamic.mjs
cargo run --locked -- examples/modules/builtins.mjs
cargo run --locked -- -e 'console.log("answer", 6 * 7)'
cargo run --locked -- --help
```

`console.log` converts each argument using JavaScript string conversion, joins
them with spaces, and writes one line. It has no Node-style object inspection or
format substitutions. File execution reports errors on stderr and exits with
status 1; successful scripts exit with status 0. Script return values are ignored.

## Check

Checks run locally. GitHub Actions is manual-only and is not part of the current
development gate.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

See the [Phase 0 progress](docs/phase-0.md) and
[runtime ownership design](docs/runtime-design.md), the [supported API](docs/api.md),
the [async bridge walkthrough](docs/async-runtime.md), and
[callback timer design](docs/timers.md).
Module behavior and limits are covered in the [module walkthrough](docs/modules.md)
and [Phase 1 progress](docs/phase-1.md).
Native module exports are documented in [built-in modules](docs/builtins.md).

## Roadmap

1. Core: V8 execution, Tokio, native logging, errors and stack traces, microtasks,
   async Promises, timers, and uncaught error handling.
2. Modules: ESM, resolution, CommonJS, caching, dynamic imports, and `rustyjs:*`.
3. Minimum Node surface: events, buffers, streams, filesystem, crypto, process,
   networking, and web APIs.
4. AI: isolate permissions and limits, pooling, inference, and MCP hosting.
5. Blockchain: native numeric types, RPC, codecs, subscriptions, and signing.
6. Data science: Polars, Arrow, file readers, lazy execution, and statistics.
7. GPU: wgpu baseline, then CUDA exploration.

## Development

Implementation assistance is authorized. Explanations and review remain part of
development; see [agent instructions](AGENTS.md). Changes use feature branches and
pull requests into protected `main`.
