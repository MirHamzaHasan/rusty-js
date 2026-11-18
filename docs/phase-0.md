# Phase 0: core engine

## Baseline

The checkout started empty. On 2026-11-17 Hamza removed the human-written-only
restriction and authorized implementation. The baseline is now implemented on
`feat/phase-0-runtime`, using V8 152.2.0 and Tokio 1.54.1 with `rt` and `time`.
The [runtime design](runtime-design.md) records ownership decisions and future
regression checks.

| Step | Task | Evidence/status |
| --- | --- | --- |
| 1 | Embed V8 and run inline JS | Implemented; CLI test passes |
| 2 | Execute a `.js` file from the CLI | Implemented; example and CLI test pass |
| 3 | Add Tokio alongside V8 | Implemented; timer and context recovery test passes |
| 4 | Register `console.log` | Implemented; argument conversion and error tests pass |
| 5 | Production errors and `ScriptOrigin` stack traces | Initial exception boundary implemented and tested; fatal V8/resource-limit recovery remains outside this baseline |
| 6 | Pump microtask checkpoints | Implemented with explicit policy and chained-reaction tests |
| 7 | Resolve a JS Promise after async Tokio work | Implemented with Rusty.sleep, task identity, failure, and cancellation tests |
| 8 | Timers using Tokio | Implemented: timeout/interval registration, clearing, callback arguments, and fixed-delay interval policy |
| 9 | Uncaught exceptions and unhandled rejections | Initial handling implemented; first unhandled rejection fails the run at a checkpoint; no Node rejection-event compatibility |

## Implementation walkthrough

1. `src/main.rs` parses file/eval/help/version arguments before initializing V8.
   File paths remain OS strings, so they do not need to be valid UTF-8.
2. `run` reads the source and constructs a current-thread Tokio runtime with its
   time driver enabled. `block_on` drives the root future on that same thread.
3. `JsRuntime::new` initializes the process-wide V8 platform once, creates an
   isolate and context, and installs the native logger. V8 initialization is kept
   alive for process lifetime; dropping one runtime must not shut down all others.
4. The context is retained by a `Global` handle. It is declared before the isolate
   so Rust drops it first. All scope-bound handles remain inside synchronous calls.
5. `execute` enters the context and a `TryCatch`, supplies the filename through
   `ScriptOrigin`, and compiles/runs the classic script. Exceptions become owned
   Rust diagnostics rather than escaping handles or panics.
6. `console_log` converts arguments before locking stdout, so guest conversion
   code can throw or log recursively. Failed conversion leaves the JS exception
   pending. Output failures become a JS Error.
7. The CLI reports a diagnostic and exits with failure. A library caller can catch
   `JsError` and execute another script using the same context, as the test proves.

## Validation on 2026-11-17

Local environment: macOS aarch64, Rust/Cargo 1.97.1.

- `cargo check`: passed against the actual V8 archive.
- `cargo test --locked`: passed, one library test and five CLI integration tests.
- `cargo run --locked -- examples/hello.js`: printed `Hello from RustyJS 42`.
- `cargo fmt --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.

The library test verifies context persistence over a Rust timer, runtime recovery
after thrown errors and invalid syntax, a filename/line in diagnostics and stack,
and thrown objects whose string conversion is absent or itself throws. CLI tests
exercise output, file execution, failed logging conversion, invalid arguments,
missing files, help, version, and normal failure exit codes.

Linux/macOS/Windows CI is configured in `.github/workflows/ci.yml`; no remote CI
run is claimed. This is local validation on one platform, not cross-platform proof.

## Async bridge update on 2026-11-18

V8 now uses explicit checkpoints after top-level execution and each completed host
operation. `Rusty.sleep(ms)` retains a Promise resolver on the isolate thread,
queues Rust-only work, and lets Tokio drive its timer. The event loop settles
completed operations by identity, handles work created by Promise reactions, and
returns when no host operations remain. It also detects unhandled rejections at
the checkpoint boundary. See the [API](api.md) and [code walkthrough](async-runtime.md).

The test suite now includes six library tests and ten CLI integration tests.
The new cases cover out-of-order completion, failed/panicking tasks, cancellation,
cleanup and reuse, Promise reaction ordering, handled versus unhandled rejection,
delay validation without object coercion, and definition-file diagnostics for
callbacks invoked from another script. The original baseline validation
above remains a historical record; the update's final checks are recorded below.

Validated locally on macOS arm64 with Rust/Cargo 1.97.1:

- `cargo test --locked`: all 16 tests passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo fmt --check`: passed.
- `cargo doc --locked --no-deps`: generated the Rust API reference successfully.
- `cargo run --locked -- examples/async.js`: printed the two synchronous lines
  before `Tokio task finished`, then exited normally.
- Markdown links and whitespace: valid across the six project documents.

The failure tests exposed V8's unavailable-column sentinel (`usize::MAX`), which
previously overflowed when converted to a one-based column. Diagnostics now keep
the column absent in that case. Diagnostics also prefer the filename supplied by
V8, so a callback defined in one script is not attributed to its caller's file.

## Callback timer update on 2026-11-18

All four callback timer globals are implemented with callback/argument roots on
the isolate thread. The event loop waits for the earliest live deadline while
continuing to process host Promise completions. Intervals use a fixed delay after
callback and checkpoint completion, clamp zero periods to 1 ms, and skip missed
ticks. Clearing a timer removes it from the live registry even if its deadline
has already passed. See the [API](api.md) and [timer walkthrough](timers.md).

The suite now has nine library tests and fifteen CLI integration tests. New
coverage includes callback arguments/receiver, callback-to-microtask order,
queued/ready timer cancellation, cross-clearing, interval self-cancellation,
microtask cancellation, input validation, callback errors, and root cleanup.
Paused-clock tests verify the missed-tick policy without precise wall-clock
assertions and verify that async interval work drains after clearing. A zero-delay
chain test checks host Promise completion progress.

Validated locally on macOS arm64 with Rust/Cargo 1.97.1:

- `cargo test --locked`: all 24 tests passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo fmt --check`: passed.
- `cargo doc --locked --no-deps`: generated the Rust API reference successfully.
- `cargo run --locked -- examples/timers.js`: printed `timeout 42` and three ticks,
  omitted the cleared callback, and exited normally.
- Markdown links and whitespace: valid across the seven project documents.

## Phase 1 handoff

The initial Phase 0 surface is in place. Phase 1 now includes file/package ESM,
dynamic imports, native modules, package maps, and bounded CommonJS loading.
See [Phase 1 progress](phase-1.md) for current evidence and remaining boundaries.
Broader Node compatibility remains later work. Validation currently runs locally;
the local evidence above does not establish Windows or Linux support.

A current-thread runtime does not remove `tokio::spawn`'s `Send` bound. A `LocalSet`
is unnecessary for the current root future. Synchronous JS still occupies the
thread until it returns; execution timeouts and memory caps are not implemented.

## Bridge invariants

Keep resolver ownership on the V8 thread using a persistent handle. Async work
returns ordinary Rust values and an operation identifier; the runtime drains
completions, re-enters V8, settles the corresponding Promise, and runs microtasks.
Design errors, cancellation, idle detection, and teardown before calling it done.
An unresolved Promise by itself is not an active host operation.

## References

- [Tokio runtime Builder](https://docs.rs/tokio/latest/tokio/runtime/struct.Builder.html):
  read `new_current_thread`, `enable_time`, and `build`.
- [Tokio LocalSet](https://docs.rs/tokio/latest/tokio/task/struct.LocalSet.html):
  read why ordinary spawning still requires `Send` and how local tasks are driven.
- [deno_core event loop reference](https://github.com/denoland/deno_core/blob/main/core/runtime/jsruntime.rs):
  study `JsRuntime::poll_event_loop` for the later Promise bridge. Treat this as a
  reference implementation, not code to paste; check the pinned dependency API.
