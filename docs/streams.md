# Streams: Readable/Writable foundation

The stream module provides named `Writable` and `Readable` exports from `rustyjs:stream`,
`node:stream`, and `stream`. Aliases share one ESM namespace and constructor.
CommonJS returns a cached object containing those constructors and the
[finished/pipeline helpers](stream-lifecycle.md). Writable extends the existing
[EventEmitter](events.md) and uses [Buffer](buffer.md) for byte chunks.
There is no global stream class or default ESM export yet.
This page covers Writable. See [Readable and piping](readable.md) for pulls,
buffered reads, flow controls, iterable sources, and pipe cleanup.

```js
import { Writable } from 'node:stream';

const chunks = [];
const sink = new Writable({
  highWaterMark: 4,
  write(chunk, encoding, done) {
    setTimeout(() => {
      chunks.push(chunk.toString());
      done();
    }, 1);
  },
});

sink.on('error', error => console.log(error.message));
sink.on('finish', () => console.log(chunks.join('')));
if (sink.write('hello ')) sink.end('world');
else sink.once('drain', () => sink.end('world'));
```

Run `cargo run --locked -- examples/streams/writable.mjs`.

## Queue and backpressure

`write(chunk[, encoding][, callback])` queues one write and returns a Boolean.
Only one `_write` hook is active at a time. The queue includes the in-flight
chunk, so `writableLength` counts it until its callback completes. Byte mode
counts encoded bytes; object mode counts chunks. The default high-water mark
is 65,536 bytes or 16 objects. An explicit highWaterMark must be a primitive
integer Number between zero and 64 MiB.

A return value of false means the producer should wait for `drain` before
writing more. `writableNeedDrain` is cleared when the outstanding length reaches
zero and drain is emitted. Ending/destroyed streams do not emit a pending drain.
The high-water mark is advisory, not an allocation ceiling. Ignoring false can
grow the queue without bound; this is not a sandbox memory budget.

`cork()` increments a nesting count. `uncork()` releases one level and starts
queued writes when the count reaches zero. `end` flushes all cork levels. There
is no writev batching: each record reaches `_write` separately.

## Chunks and hooks

Byte streams accept strings, Buffers, or Uint8Arrays. Strings use the supplied or
default encoding, decode into Buffer, and reach `_write` with encoding `buffer`.
Existing Buffers retain their identity and share their bytes with the caller;
plain Uint8Arrays are copied. Detached byte inputs throw before queue mutation.
Do not mutate a submitted Buffer until its write callback runs.

Object mode preserves chunk identity, including undefined. Both modes reject null;
byte mode also rejects undefined.
With decodeStrings=false, strings reach `_write` unchanged and their queue cost
is UTF-16 string length. `setDefaultEncoding(name)` changes subsequent string
writes. Encodings use the Buffer contract; invalid names/types fail synchronously.

Subclass hooks or constructor options can provide:

- `_write(chunk, encoding, callback)` / `write`: process one chunk; call the
  callback when done. A missing hook fails with ERR_METHOD_NOT_IMPLEMENTED.
- `_final(callback)` / `final`: flush after writes and their callbacks settle.
  The default hook succeeds immediately.
- `_destroy(error, callback)` / `destroy`: release external resources, cancel
  timers/operations owned by the stream, then call back. The default completes
  immediately with the supplied error.

Hook receiver is the stream. Returning a Promise does not replace the required
callback. Null/undefined callback reasons mean success; other reasons are errors.
Calling a write/final callback twice destroys the stream with ERR_MULTIPLE_CALLBACK;
an earlier reported error keeps priority. Later completions after destruction are
ignored. Repeated destroy cleanup callbacks are ignored to preserve idempotence.

## Completion, errors, and destruction

`end([chunk[, encoding]][, callback])` marks writableEnded immediately and rejects
further writes. After the final queued write's callback, `_final` runs, then
prefinish and finish are emitted. End callbacks follow finish. By default the
stream then destroys itself and emits close after `_destroy` finishes. Options
autoDestroy=false and emitClose=false independently control automatic destruction
and the close event. The closed flag still becomes true when emitClose=false.
Repeated end calls can register completion callbacks without repeating finish.

Write/final failures destroy the stream. Every outstanding write callback and end
callback is settled once, queued chunks are released, and finish is suppressed.
Pending callbacks receive the original error, or ERR_STREAM_DESTROYED for a manual
error-free destroy. After cleanup, an error event precedes close. A write after
end returns false and fails with ERR_STREAM_WRITE_AFTER_END; a write on an already
destroyed stream only fails its callback without repeating lifecycle events.

`destroy(error)` is idempotent, immediately marks destroyed, clears queue length,
and invokes cleanup once. It cannot automatically cancel work created by a custom
hook: use `_destroy` to cancel its timer or native operation. A hook that never
calls back can stall completion. Pure pending JS stream state does not keep Tokio
alive. Runtime-wide host cancellation does not synthesize per-stream destruction.

Observable getters include writable, writableLength, writableHighWaterMark,
writableObjectMode, writableNeedDrain, writableCorked, writableEnded,
writableFinished, writableAborted, destroyed, closed, and errored. State lives in
a private WeakMap, without an exposed Node `_writableState` structure.

## Scheduling and implementation

1. Fresh-context setup captures EventEmitter and Buffer before guest code runs,
   compiles the bundled stream function, and roots its constructor in the
   [built-in registry](builtins.md). No mutable guest global lookup supplies it.
2. An idle write invokes `_write` immediately. Completion processing, user write
   callbacks, drain, finish, end callbacks, and close use captured Promise
   reactions, drained by the runtime's existing explicit microtask checkpoints.
3. A completion phase prevents reentrant writes or end from starting another
   hook before the completed write's callback. Once it runs, the next queued
   record starts. Separate notification jobs allow cleanup to continue even if
   a listener or callback throws.
4. Throwing user notification code becomes an unhandled Promise rejection under
   the existing runtime policy. An unhandled error event likewise fails the run.
   Hook throws are converted into stream failure and destruction.
5. Everything above runs in V8 on the isolate thread. Async hooks can use existing
   Tokio-backed timers/operations; no V8 handle crosses a worker boundary.
   No separate Rust stream registry, scheduler, or performance claim is added.

Promise checkpoint scheduling differs from Node's nextTick phases, including the
specified finish/end-callback order. Internal lifecycle events use the captured
EventEmitter dispatch method. Captured intrinsics are API stability measures,
not a hardened realm or preemption mechanism. A long microtask chain can delay
Tokio progress.

Duplex/Transform and web
streams, AbortSignal, `_construct`, `_writev`, and broad Node option/internal
compatibility are not implemented yet. Known unsupported construct/writev/signal
options throw rather than silently succeeding. Unknown options are ignored.
Package compatibility must eventually be measured with actual package suites.

The selected API shape follows the
[Node v24.11.0 streams reference](https://nodejs.org/download/release/v24.11.0/docs/api/stream.html),
with the explicit scheduling and scope differences above.

## Local validation

Sixteen stream integration tests cover aliases/inheritance, queue serialization,
backpressure, reentrant drain, corking, encodings, object mode, async hooks,
finalization, repeated end, cleanup failures, duplicate callbacks, original errors,
detached inputs, lifecycle options, validation, and unhandled callback/listener
errors without skipped cleanup. Tests run locally on macOS arm64; other platforms
remain unvalidated and GitHub Actions stays idle.
