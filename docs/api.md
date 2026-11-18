# Supported API

This is the implemented Phase 0 surface. Planned Node APIs and future AI,
blockchain, dataframe, and GPU modules are not available yet.

## CLI

```sh
cargo run --locked -- examples/hello.js
cargo run --locked -- examples/async.js
cargo run --locked -- examples/timers.js
cargo run --locked -- examples/modules/main.mjs
cargo run --locked -- -e 'Rusty.sleep(10).then(() => console.log("done"))'
```

| Form | Behavior |
| --- | --- |
| `rustyjs file.js` | Execute one UTF-8 classic script |
| `rustyjs file.mjs` | Load and evaluate a static local ES module graph |
| `rustyjs --module file` or `-m file` | Select module mode explicitly, including for a `.js` entrypoint |
| `rustyjs -e source` or `--eval source` | Execute inline source using `<eval>` as its origin |
| `rustyjs -- file.js` | Treat the following argument as a filename, including a leading `-` |
| `rustyjs --help` or `-h` | Print usage |
| `rustyjs --version` or `-v` | Print the runtime version |

The CLI runs the script, drains its Promise reactions, and drives registered host
operations until idle. Success exits with status 0. Invalid arguments, failed file
reads, uncaught synchronous exceptions, and unhandled Promise rejections produce
stderr diagnostics and status 1. It reports the first failure for that turn.

Script return values are ignored. Additional script arguments, stdin execution,
CommonJS, and a REPL are not implemented.

## ES modules

Static imports accept explicit `./` / `../` paths or native OS absolute paths and
resolve relative to the importing file. Files are canonicalized for identity and
must exist; there is no extension/index guessing, file-URL decoding, bare-package
lookup, or package.json format inference. Imported files are parsed as JavaScript
modules regardless of extension. Import attributes are rejected.

Module graphs support default/named exports, re-exports, live bindings, cycles,
and top-level await through the host Promise/timer loop. Module success and
evaluation failure are cached per runtime. Source edits need a fresh runtime to
take effect. Compile/link failures roll back newly prepared graph entries so a
fixed source can be retried before evaluation.

A pending module evaluation with no host operations or timers is reported as an
error rather than a successful exit. Dynamic imports and `import.meta` host
metadata are not available yet. See the [module contract](modules.md).

## `console.log(...values)`

Each value undergoes JavaScript string conversion; the resulting strings are
joined by a space and written as one stdout line. No arguments produce a blank
line. Object inspection, `%s` substitutions, and other console methods are absent.

Conversion can execute guest code. If it throws, the log call throws and writes
none of its own line. A failed stdout write also throws a JavaScript Error.

## `Rusty.sleep(milliseconds): Promise<void>`

```js
console.log("before");
Rusty.sleep(10).then(() => console.log("after"));
```

The argument must be a primitive Number containing a finite integer from 0 to
2147483647, inclusive. Other values throw a TypeError synchronously. Strings and
objects are not coerced, so validation does not call `valueOf` or `toString`.

The delay deadline is recorded when the native function is called. Tokio drives
the timer after control returns to the host; synchronous JS is not preempted.
Even a zero delay resolves asynchronously after the script and its initial
microtask checkpoint. Timer precision is subject to Tokio and OS scheduling.

The Promise fulfills with `undefined`. A host task failure rejects it with Error.
Promise reactions may register additional sleeps; the host continues until they
finish. The host keeps referenced sleep operations alive even if their returned
Promise is not stored. A JS-only pending Promise does not keep it alive.

This is a RustyJS host API. Top-level `await` works in module mode; use `.then()`
or an invoked async function in a classic script.

## Callback timers

```js
const timeout = setTimeout((message) => console.log(message), 0, "done");
const interval = setInterval(() => console.log("tick"), 100);
clearInterval(interval);
```

| Function | Contract |
| --- | --- |
| `setTimeout(callback, delay = 0, ...args)` | Invoke once after its deadline and return a positive safe-integer ID |
| `setInterval(callback, delay = 0, ...args)` | Repeat with fixed delay after each callback and checkpoint; effective period is at least 1 ms |
| `clearTimeout(id)` | Remove either kind of callback timer; unknown or invalid IDs are ignored |
| `clearInterval(id)` | Same cancellation contract as `clearTimeout` |

The callback must be a function; strings are not evaluated. Omitted or `undefined`
delay means zero. Any supplied delay must otherwise be a primitive Number holding
a finite integer in 0..2147483647. Invalid callbacks or delays throw TypeError
without object/string coercion. Clear functions ignore non-number or non-positive,
fractional, non-finite, and unsafe IDs without coercion.

Callbacks receive the extra arguments without string conversion and use
`globalThis` as their receiver. Their return values are ignored. Promise reactions
run after each callback, before another timer callback. An interval can clear
itself directly or from a reaction in that checkpoint. Missed interval ticks are
skipped; synchronous invocations do not overlap and no catch-up burst is queued.
Returned Promises are not awaited before an interval rearms, so async callback
work may overlap. Its registered host operations still drain before idle exit.

Timers are ordered by deadline, then ID for an exact tie. Deadlines do not preempt
running JS and are not precise execution guarantees. Timers keep the runtime alive
until they fire or are cleared. Clearing the last long timer allows immediate idle
exit. Node-style Timeout objects, `ref`/`unref`, and string callbacks are absent.
The [timer walkthrough](timers.md) explains scheduling and cleanup.

## Rust library

| Member | Contract |
| --- | --- |
| `JsRuntime::new()` | Initialize V8 once per process, then create a private isolate and context |
| `execute(source, origin)` | Compile/run a classic script and complete its microtask checkpoint; queue host work without waiting for it |
| `execute_module(path)` | Prepare/link/evaluate a static local module graph and checkpoint its initial reactions; queue host work needed by top-level await |
| `run_event_loop().await` | Start queued Rust futures, settle their Promises on the isolate thread, and continue until no host work remains |
| `cancel_pending()` | Discard queued host work and callback timers, releasing their persistent roots before the event loop starts |
| `JsError` | Owned message, origin, optional line/column, and optional stack text |

Call the event loop inside Tokio with its time driver enabled; the CLI uses a
current-thread runtime. Keep the isolate on its creating thread. Multiple runtimes
on one thread must be dropped in reverse creation order because of V8's entered
isolate stack. V8 platform initialization remains alive until process exit.

Dropping a running event-loop future aborts its tracked Tokio tasks and releases
pending resolver and timer callback/argument roots. Aborted async tasks are dropped when Tokio next polls
their cancellation; none of these tasks can touch V8. Cancellation leaves the
corresponding JavaScript Promises pending. A caller retaining those Promises is
responsible for its own application-level cancellation semantics. Module-cache
handles remain cached until the runtime is dropped; a cancelled async module may
remain pending and needs a fresh runtime to restart its graph.

An execution or event-loop error cancels outstanding host work. The caller can
inspect the error and reuse the context. Existing guest state and side effects
are retained; error recovery is not a transaction rollback.

## Rejection policy and limits

Rejections are candidates until the current microtask checkpoint ends. Attaching
a handler within that checkpoint removes the candidate. A handler attached in a
future host turn is too late: the runtime has already reported the error and
stopped that run. This policy is deliberately small and is not full Node rejection
event compatibility. Rejection diagnostics use V8's message/location when
available; full stack text is not guaranteed.

The runtime currently executes trusted scripts. It has no memory cap, execution
deadline, permissions model, infinite-loop interruption, or microtask budget.
Tokio scheduling cannot interrupt synchronous JS or an endless microtask chain.
