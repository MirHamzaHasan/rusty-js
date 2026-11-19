# Readable: pulling, buffering, and pipes

`Readable` is a named export alongside Writable from `rustyjs:stream`,
`node:stream`, and `stream`. ESM aliases share namespace/constructor identity;
CommonJS returns one cached object containing both constructors. Readable extends
the same [EventEmitter](events.md) and uses the existing [Buffer](buffer.md).

```js
import { Readable, Writable } from 'node:stream';

const source = Readable.from(['hello ', 'world'], { objectMode: false });
const sink = new Writable({
  highWaterMark: 4,
  write(chunk, encoding, done) {
    setTimeout(() => { console.log(chunk.toString()); done(); }, 1);
  },
});
source.on('error', error => { sink.destroy(); console.log(error.message); });
sink.on('error', error => { source.destroy(); console.log(error.message); });
source.pipe(sink);
```

`cargo run --locked -- examples/streams/pipe.mjs` demonstrates an asynchronous
sink, backpressure, explicit peer cleanup on errors, and top-level await.

## Producer contract

Provide `read(size)` in constructor options or override `_read(size)`. The hook
runs with the stream as receiver, starts an operation, and supplies chunks using
`this.push(chunk[, encoding])`. `_read` is not called again while a pull is
outstanding. A nonempty push releases that pull; an empty byte chunk does not.
Returning a Promise does not replace push. A missing hook fails with
ERR_METHOD_NOT_IMPLEMENTED. Hook throws become stream errors and destruction.

`push(null)` marks EOF and returns false. End is emitted only after buffered data
is consumed and a consumer has requested progress. A later non-null push destroys
the stream with ERR_STREAM_PUSH_AFTER_EOF. Pushes after destruction return false.
Repeated EOF is harmless.

Byte mode accepts strings, Buffer, and Uint8Array. Strings use Buffer encodings;
Buffers retain shared storage and plain Uint8Arrays copy. Empty strings, empty
byte views, and undefined are empty byte input. Object mode preserves arbitrary
values, including undefined, false, zero, and empty strings; null remains EOF.
Writable object mode now accepts undefined too, so these values can be piped.
Other typed-array/DataView chunk formats are outside this subset.

Push returns true while the queue is below its high-water mark, false otherwise.
Defaults are 65,536 bytes or 16 objects. An explicit highWaterMark must be a
primitive integer Number in 0..64 MiB. This is advisory: a producer that ignores
false can grow memory without bound. Paused consumers may still prefetch up to
the high-water mark after consumption has started. Pure stream state does not
keep Tokio alive; the producer's actual host operation must supply progress.

## Consumer modes

Choose a single consumption style per source:

- Add a `readable` listener and call `read([size])`. Unsized byte reads consume
  the current buffered bytes; sized reads return null until enough bytes arrive,
  or the available remainder after EOF. Object reads take one item. `read(0)`
  requests progress without consuming. A successful read also emits data.
- Add a `data` listener for flowing chunks. Flow dispatches one queued chunk per
  microtask job, preserving producer chunk boundaries unless a previous manual
  read split a chunk. `pause()` stops flowing; `resume()` starts it again.
  Resuming without a data listener discards chunks. Listener variants on,
  addListener, once, prependListener, and prependOnceListener all start flow.
- Use `pipe(destination)` to connect to Writable and manage backpressure.

Adding a readable listener selects manual consumption and prevents automatic
flow. Remove it and call resume explicitly before changing styles. Don't mix
manual reads with pipes/data listeners: manual reads also dispatch data and can
therefore feed a pipe independently of its flow gate.

Read sizes are primitive integer Numbers in 0..64 MiB. Requesting more than the
current high-water mark raises it to the requested size, rather than Node's
rounded growth rule. Split byte reads return shared Buffer views; reads spanning
chunks copy into a new Buffer. Unsized aggregation beyond Buffer's 64 MiB limit
fails the stream; read smaller regions when buffering more than that. Don't mutate
or detach an input Buffer until consumed. Detachment while queued causes a
controlled TypeError stream failure, not native pointer access.

`unshift(chunk[, encoding])` prepends unread data. It can put data back after EOF
but before end, delaying end until that data is consumed. After end it fails with
ERR_STREAM_UNSHIFT_AFTER_END_EVENT. `unshift(null)` signals EOF.

## Pipes and errors

`pipe(destination[, {end: true}])` returns the destination. A destination must be
an initialized EventEmitter with write/end methods and compatible deferred drain
behavior. Writable supplies this contract. Pipe installation emits pipe on the
destination. Repeated pipe calls install distinct connections.

When any destination write returns false, flow pauses. It resumes after **every**
blocked connection drains, provided the user has not manually paused. All
destinations receive the current data event before the flow gate stops the next
one. Detaching a blocked destination removes its contribution to the gate.

EOF calls destination.end once per connection unless end=false. Pipe listeners
are removed on EOF, unpipe, source destruction, or destination finish/close/error.
`unpipe(destination)` removes matching connections; omitted destination removes
all. Removing the last pipe pauses the source. The destination receives unpipe
with the source and a `{hasUnpiped: true}` information object.

Pipe is **not** a pipeline supervisor. A destination error detaches that
connection without destroying the source. Source destruction detaches pipes
without destroying destinations. Handle errors on both sides and explicitly
clean up the peer, as the example does. The internal destination error listener
does not swallow unhandled failures: with no user error listener, they still fail
the run. Synchronous destination write failures destroy that destination when it
provides destroy; otherwise they emit an error after unlinking.

Listener metadata callbacks can throw or destroy a stream during installation.
Setup rolls back partially installed bindings. Teardown removes registry identity
before invoking guest metadata callbacks, completes all remaining removals even
if one throws, and reports those throws through the existing rejection policy.
This prevents reentrant destroy/unpipe from revisiting the same connection.

## Iterable sources

`Readable.from(iterable[, options])` accepts synchronous or asynchronous iterables
and defaults to objectMode=true. A string or Buffer is one chunk, rather than an
element-by-element iterator. Iterator values may be Promises; null values fail
with ERR_STREAM_NULL_VALUES. Empty byte chunks are skipped in byte mode without
stalling subsequent elements.

Only one iterator next/value operation is in flight. Stream options are validated
before the iterator factory is called. A pull/result/value failure destroys the
source, and early destruction invokes iterator.return once when present. Cleanup
is awaited before close; the original failure takes priority over a cleanup
failure. Late next results after cancellation are ignored. Natural iterator EOF
does not call return. The factory owns read/destroy hooks, so options supplying
those hooks are explicitly rejected.

Calling return cannot preempt arbitrary JS or cancel an unrelated pending Promise.
A generator blocked on external work may delay cleanup; its implementation must
provide cancellation. Consuming Readable with for-await is a separate upcoming
slice; support for async iterable producers does not imply async iterator output.

## Lifecycle and implementation walkthrough

`destroy(error)` is idempotent: it immediately releases queued chunks, disables
flow and pulls, and unlinks pipes. Override `_destroy(error, callback)` or use the
destroy constructor option to cancel producer-owned host work. Calling its
callback emits error when applicable, then marks closed and emits close unless
emitClose=false. The closed flag still updates with emitClose=false. Natural end
auto-destroys unless autoDestroy=false. A cleanup hook that never calls back can
stall close, and runtime-wide cancellation does not synthesize stream destruction.

Getters include readable, readableLength, readableHighWaterMark, readableObjectMode,
readableFlowing (initially null), readableEnded, readableAborted, readableDidRead,
destroyed, closed, and errored. readableEncoding is null because setEncoding and
incremental text decoding are not implemented yet.

1. Fresh-context setup compiles separate bundled Writable/Readable bodies with
   captured EventEmitter and Buffer constructors, then roots both exports in the
   existing synthetic module registry. Guest global replacement is independent.
2. A private WeakMap holds each queue, pull status, EOF state, and connections.
   Chunks remain V8 objects; no per-chunk Rust registry or new scheduler is added.
3. Manual reads can invoke a synchronous pull. Notifications and flowing work use
   captured Promise reactions at existing V8 checkpoints. Internal jobs discard
   return values, so an incidental stream `.then` property is not assimilated.
4. Data events from manual reads are synchronous. Flowing dispatch, readable,
   pause/resume, unpipe, EOF, and close jobs are deferred. This differs from Node's
   nextTick/event phases and some synchronous push behavior.
5. Notifications are separate jobs and schedule continuation before guest calls.
   Listener throws still fail the run, while teardown can finish. Captured
   intrinsics provide selected API stability, not a hardened realm or preemption.

There is no setEncoding/StringDecoder, async consumer iteration, Duplex/Transform,
pipeline/finished, web-stream adapter, AbortSignal, `_construct`, or native IO
adapter yet. No full Node compatibility or speedup is claimed. The selected API
shape follows the [Node v24.11.0 stream reference](https://nodejs.org/download/release/v24.11.0/docs/api/stream.html),
with the explicit limits and scheduling differences above.

## Local correctness evidence

Twenty-three Readable tests cover copying/splitting, read sizes, object values,
pull serialization, flow controls, multiple destinations, unpipe, EOF, errors,
iterators, late completions, captured intrinsics, metadata re-entry, rollback,
and cleanup after listener failures. They run alongside the sixteen Writable
tests. Local macOS arm64 validation does not establish Windows/Linux support.

A deterministic corpus produced 256 identical entries with local Node v20.19.4,
covering byte reads, push pressure, and asynchronous piping. This is selected
correctness evidence, not tick-order parity, package compatibility, or performance.

```sh
cargo build --locked
node tests/fixtures/readable-conformance.cjs > /tmp/rustyjs-readable-node.json
target/debug/rustyjs tests/fixtures/readable-conformance.cjs > /tmp/rustyjs-readable-runtime.json
cmp /tmp/rustyjs-readable-node.json /tmp/rustyjs-readable-runtime.json
cargo test --locked --test readable --test streams
```
