# EventEmitter

The first Phase 2 surface is a synchronous EventEmitter. `rustyjs:events`,
`node:events`, and bare `events` resolve to one built-in identity before package
lookup. Other Node built-ins are not implied by these aliases.

```js
import EventEmitter, { EventEmitter as NamedEmitter } from 'node:events';
const bus = new EventEmitter();
bus.once('ready', value => console.log(value));
bus.emit('ready', 42);
```

Both ESM exports refer to the same constructor. CommonJS `require('events')`
returns that constructor directly, with `.EventEmitter` referring to itself.
Static/dynamic imports, private package aliases, and created require functions
share the constructor. No global EventEmitter is installed.
Direct require.resolve calls preserve the requested registered alias; private
package redirects may report the canonical rustyjs:events ID.

## Supported contract

| API | Behavior |
| --- | --- |
| `new EventEmitter([options])` | Initialize an emitter; subclassing and EventEmitter.call(this) are supported |
| `on` / `addListener` | Append a function listener, allow duplicates, and return the emitter |
| `prependListener` | Insert before existing listeners |
| `once` / `prependOnceListener` | Remove and mark the wrapper fired before invoking the original callback |
| `emit(name, ...args)` | Call a snapshot synchronously in order with the emitter as receiver; return whether listeners ran |
| `off` / `removeListener` | Remove the most recently registered matching function, including a once original |
| `removeAllListeners([name])` | Clear one event or all events; notify removeListener observers in reverse registration order |
| `listeners(name)` | Return a copy with original once functions |
| `rawListeners(name)` | Return a copy retaining once wrappers; their `.listener` property identifies the original |
| `listenerCount(name[, listener])` | Count all listeners or matches for one function |
| `eventNames()` | Return own property keys: integer-like strings first, then strings, then symbols |
| `setMaxListeners(n)` / `getMaxListeners()` | Configure a nonnegative Number limit; 0 and Infinity disable warnings |
| `EventEmitter.defaultMaxListeners` | Default 10; changing it affects emitters without an explicit local limit |

Event names follow JavaScript property-key conversion, including symbols and
numeric strings. A throwing conversion propagates normally. Prototype-like names
such as `__proto__` are ordinary events because the event table has no prototype.
Listeners must be functions. Methods require an initialized emitter receiver;
they do not lazily turn arbitrary objects into emitters.

Listeners added during emit wait until the next emit. Removal during emit does
not cancel entries already in that call's snapshot. A once wrapper has a fired
flag, so nested emits or a saved raw wrapper cannot invoke it twice. Listener
return values are ignored, including Promises. An async listener can register
normal host work, but its rejection follows the runtime's
[unhandled rejection policy](api.md#rejection-policy-and-limits).

`newListener` fires before insertion with the original listener, allowing an
observer to add a listener ahead of it. `removeListener` fires after removal and
receives the original once function. These meta-events are emitted only when
observers exist. The remove-all path leaves removal observers until last.

An unhandled string `error` event rethrows an Error argument by identity. Other
values produce an Error with a `.context` property containing that value. A
registered error listener handles it normally. A listener throwing any value stops
the current dispatch and propagates that same value. A symbol described as
`error` is still an ordinary symbol event.

Listener-limit warnings are advisory: no listener is dropped. The first crossing
of a collection's limit writes a concise MaxListenersExceededWarning to stderr.
Removing down to a single listener or clearing the collection allows a later
crossing to warn again. Warning output is synchronous in RustyJS; process warning
events and Node's warning scheduling/format are not implemented.

## Implementation walkthrough

1. During fresh-context setup, Rust compiles the bundled `src/js/events.js` as a
   function with one private warning callback. It returns the constructor before
   guest code can replace the intrinsics used by the implementation.
2. The captured-export registry roots the constructor under the Events built-in
   ID. ESM synthetic modules expose default and EventEmitter names; CommonJS
   returns the same function object directly instead of a module-shaped object.
3. Per-instance state uses a closure-private WeakMap. Event tables are prototype-
   free, listener collections remain in V8, and introspection returns array copies.
   There is no Rust listener registry and no handle crossing a worker boundary.
4. Dispatch snapshots a listener collection before invoking callbacks with a
   captured Reflect.apply. Once wrappers remove themselves before user code runs.
   Adding after newListener notifications re-reads the current collection, which
   preserves re-entrant insertion order.
5. Captured WeakMap/array/reflect methods avoid later global method replacement
   changing the normal implementation path. This is not a hardened realm or a
   general prototype-tampering security boundary.
6. Only warning text crosses into Rust for stderr output. An IO failure becomes a
   catchable JS error; the host does not panic. All listener dispatch and storage
   run as JavaScript on V8, with no inherent JS execution speed advantage claimed.

An emitter with attached listeners does not itself keep the Tokio loop alive.
Timers or other host work must drive later emits. Runtime cancellation cancels
those host operations; it does not clear still-referenced guest emitter objects.

## Deliberate limits

Promise rejection capture is unsupported. Requesting captureRejections=true in
constructor options or on the constructor throws TypeError; false is accepted.
Promise-based events.once/events.on helpers, AbortSignal integration, async
iterators, errorMonitor, async-resource integration, EventTarget, and internal
Node fields such as _events are not implemented. The `.listener` property on a
once wrapper is introspection metadata; editing it does not replace the original
callback retained by the wrapper.

This is a tested core API subset, not proof of broad Node package compatibility.
The behavior targets the core contracts in the
[Node 24 events documentation](https://nodejs.org/download/release/v24.11.0/docs/api/events.html),
with the limits above kept explicit.

## Run and verify locally

```sh
cargo run --locked -- examples/events/main.mjs
cargo run --locked -- examples/events/main.cjs
```

They print `event: 42` once and `CommonJS event: 42` respectively. The event suite
covers module aliases, constructor identity, legacy/subclass initialization,
receiver/arguments/order, mutation snapshots, duplicate removal, once re-entry,
meta-events, symbols/keys, error identity and dispatch termination, ignored async
returns, listener warnings, validation, and captured intrinsic methods.
