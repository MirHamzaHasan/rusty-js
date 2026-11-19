# Async stream iteration and lifecycle helpers

Readable now supports `for await`, and the stream modules provide completion and
pipeline supervision. These use the existing V8 Promise checkpoints and registered
RustyJS stream states. No new native IO adapter, scheduler, permission boundary,
or performance claim is added.

| Module aliases | Named exports |
| --- | --- |
| node:stream / stream / rustyjs:stream | Readable, Writable, finished, pipeline, promises |
| node:stream/promises / stream/promises / rustyjs:stream/promises | finished, pipeline (Promise forms) |

There is no default ESM export. `require('stream').promises` and
`require('stream/promises')` are the same ordinary object. ESM bindings capture
the original function values, so assigning a property on that object does not
rewrite an already captured module binding. Aliases share namespace/function
identity within one runtime.

## Async consumers

```js
import { Readable } from 'node:stream';

const source = Readable.from([1, 2, 3], { highWaterMark: 1 });
for await (const value of source) {
  console.log(value);
  if (value === 2) break;
}
console.log(source.closed); // true after default iterator cleanup
```

Iterator construction is lazy: it does not pull data until next is requested.
Concurrent `next()` calls receive queued values in FIFO order. Object mode keeps
undefined and other falsy values; byte mode uses the existing unsized read rule,
so adjacent buffered bytes can be combined into one Buffer. Choose one consumer
style rather than mixing iterators with manual reads, data listeners, or pipes.

Natural completion waits for auto-destruction cleanup when autoDestroy=true.
With autoDestroy=false, it completes after end without forcing destruction.
Early break or `iterator.return(value)` destroys by default, resolves pending
next calls as done, and waits for `_destroy` before resolving the return result.
This also works when emitClose=false. A producer error or `iterator.throw(reason)`
rejects pending work with the original reason and waits for cleanup. An external
error-free destruction before EOF rejects with ERR_STREAM_PREMATURE_CLOSE.
Subsequent next/return calls on a terminated iterator report done.

Use `source.iterator({destroyOnReturn: false})` to leave the source open after a
normal early return, then consume its remainder with another iterator. This does
not suppress producer failures or natural auto-destruction after EOF. Options
must be an object with a Boolean destroyOnReturn when supplied.

Iterator-owned error guards and private lifecycle observers are removed when it
terminates. This differs from implementations that retain a permanent late-error
guard: after preserving a source, its next owner is responsible for future errors.
Cancellation cannot preempt guest JS or cancel arbitrary Promises. A producer
whose cleanup never calls back, or an async generator blocked on unrelated work,
can still delay completion. Use `_destroy` or iterator.return to cancel owned work.

Run `cargo run --locked -- examples/streams/iterate.mjs` for early exit through an
async generator's finally block. It prints rows 1 and 2, `source released`, then
`closed: true`.

## finished: observe without consuming

```js
import { finished } from 'node:stream/promises';

// The producer/consumer must still drive the stream.
await finished(stream, { cleanup: true });
```

`finished(stream[, options], callback)` in the main stream module observes a
registered Readable or Writable and returns an idempotent cleanup function.
Callbacks are deferred, including for already completed streams. The Promise
form resolves with undefined or rejects with the failure. It does not read,
resume, write, or end the stream.

Normal completion means end/finish plus expected cleanup. It waits for registered
destruction to close, including when close events are suppressed. autoDestroy=false
allows completion without destroying a normally completed stream. Early close
fails with ERR_STREAM_PREMATURE_CLOSE. An original stream error takes priority
over a later cleanup error. Explicitly emitted error events are also observed;
these can report failure without destroying the source because finished is an
observer rather than a supervisor.

Options are Boolean when present:

- readable/writable, default true: select which existing direction to observe.
  Excluding the only direction completes asynchronously without consuming it.
- error, default true: observe failures and install an error guard. With false,
  error events remain the caller's responsibility; an unfinished early close
  still fails with ERR_STREAM_PREMATURE_CLOSE.
- cleanup, default false: remove the error guard automatically at settlement.
  With the default, a guard remains to absorb unexpected late error events until
  the returned cleanup function is called. Private transition observation always
  stops when settled.

The cleanup function cancels observation if called before completion and then
suppresses the callback. It never destroys the stream. Use cleanup=true for most
Promise observers to avoid retaining a guard. Unsupported signal options throw;
there is no AbortSignal API yet. Callback-form validation throws synchronously;
Promise-form validation rejects the Promise.

## pipeline: transfer and supervise

```js
import { Readable, Writable } from 'node:stream';
import { pipeline } from 'node:stream/promises';

const source = Readable.from(['hello ', 'world'], { objectMode: false });
const chunks = [];
const sink = new Writable({
  write(chunk, encoding, done) { chunks.push(chunk); done(); },
});
await pipeline(source, sink);
console.log(Buffer.concat(chunks).toString());
```

This slice supports **two endpoints**: a registered Readable or sync/async iterable
source, and a registered Writable destination. It accepts positional endpoints or
`[source, destination]`, with an optional options object. A positional array
followed by a Writable is an iterable source; a single endpoint array is the list
form. Iterables are adapted with Readable.from after destination/options validation.

`pipeline(source, destination[, options], callback)` returns the destination and
invokes the callback once. The Promise form resolves with undefined after the
transfer and expected cleanup. It uses the existing pipe backpressure contract.
Unlike bare pipe, it destroys unfinished peers after failure and waits for their
cleanup before reporting the first recorded error. Error-free premature closure
fails with ERR_STREAM_PREMATURE_CLOSE. Already ended/finished peers are preserved;
autoDestroy=false is respected for successful endpoints.

The end option defaults to true. With end=false, EOF does not end the destination:
pipeline waits for all submitted write callbacks, then leaves it open for reuse.
If the destination is independently ending or being destroyed, its actual
completion/cleanup is awaited instead. No success is reported while cleanup is
still pending. Keep endpoints exclusive to the pipeline during its run.

Both callback and Promise forms remove their guards, private observers, and pipe
bindings at settlement. This automatic cleanup differs from Node versions that
retain pipeline listeners. Setup observation is transactional: a metadata callback
failure before ownership removes installed guards without starting IO. A later
connection failure is supervised like a transfer failure. Listener-removal errors
are still reported through the runtime rejection policy without skipping cleanup
or the pipeline callback.

Transform/multi-stage/function pipelines, third-party duck-typed streams,
web streams, AbortSignal, and stream adapters are not implemented yet. A wrong
endpoint count or unsupported signal option fails validation. A cleanup hook
that never completes can keep the pipeline pending; this is not an execution
timeout or forced cancellation mechanism.

Run `cargo run --locked -- examples/streams/pipeline.mjs`; it prints `hello world`
and `closed: true true` after asynchronous writes and cleanup.

## Implementation walkthrough

1. Fresh-context setup creates a private lifecycle closure, then passes it to
   Readable, Writable, and helper function factories. Its WeakMap does not expose
   stream state on globals or public objects. These are JS objects in V8; no V8
   handles leave the isolate thread or cross an await in Rust.
2. Constructors register state. Data/EOF, write completion, errors, destruction,
   and close transitions notify observers. Notifications coalesce through existing
   Promise checkpoints and do not schedule observer work when nobody is watching.
3. The channel records the first non-null failure and its order at notification
   time. A pipeline can preserve the first endpoint failure even if both fail
   within one turn or `_destroy` later reports another error.
4. Async iterators queue next/return/throw requests and use the original read
   method. Completion waits for closed state rather than a possibly suppressed
   close event. Termination removes its guard/observer and settles every waiter.
5. finished observes direction completion, while pipeline owns transfer and peer
   failure cleanup. Both install error guards before activity and remove private
   observation before calling user completion code. Metadata throws cannot stop
   removal of other bindings.
6. Helpers are captured into synthetic modules alongside the constructors.
   stream/promises uses the same functions and CommonJS object. Captured
   intrinsics are selected API stability measures, not a hardened realm.

The selected signatures follow the
[Node v24.11.0 stream reference](https://nodejs.org/download/release/v24.11.0/docs/api/stream.html).
Tick phases, supported endpoint types, retained-listener policy, and the scoped
pipeline surface differ as specified above; full Node compatibility is not implied.

## Local evidence

Twenty-two helper integration tests cover aliases, FIFO iteration, early exit,
pending cancellation, preserved sources, original errors, premature close,
finished options/cancellation, async pipeline cleanup, end=false reuse, lifecycle
options, setup rollback, simultaneous failures, and metadata/callback exceptions.

A 384-entry deterministic corpus matched local Node v20.19.4 for selected values
and cleanup outcomes. It does not measure event-phase parity, package compatibility,
or performance. Local validation runs on macOS arm64; other platforms remain
unvalidated, and GitHub Actions stays idle.

```sh
cargo build --locked
node tests/fixtures/stream-lifecycle-conformance.cjs > /tmp/rustyjs-stream-node.json
target/debug/rustyjs tests/fixtures/stream-lifecycle-conformance.cjs > /tmp/rustyjs-stream-runtime.json
cmp /tmp/rustyjs-stream-node.json /tmp/rustyjs-stream-runtime.json
cargo test --locked --test stream_lifecycle
```
