# Built-in modules

`rustyjs:timers` exports the runtime's existing timer functions without adding a
second scheduler or changing their contracts. `rustyjs:module` exports
`createRequire` for explicit CommonJS access from ESM; see the
[factory contract](commonjs.md#require-factories-for-esm).
The [EventEmitter module](events.md) adds rustyjs:events with explicit node:events
and events aliases. It is bundled JavaScript rather than Rust listener dispatch.
The [Buffer module](buffer.md) adds rustyjs:buffer, node:buffer, and buffer aliases
with a named Buffer export shared with the global constructor. CommonJS returns
an object containing that constructor; there is no default ESM export yet.
The [stream module](streams.md) exports Writable, [Readable](readable.md), and
[finished/pipeline/promises helpers](stream-lifecycle.md) through
rustyjs:stream/node:stream/stream. Its constructor captures the existing
EventEmitter and Buffer identities. The stream/promises variants share Promise
helper functions and the object exported as stream.promises through CommonJS.
The [path module](path.md) adds path/node:path/rustyjs:path and explicit
posix/win32 subpaths. Its default/CommonJS value is the shared variant object;
named functions are captured independently. Only relative resolution crosses
into the native cwd callback.
The [OS/process modules](host.md) add os/node:os/rustyjs:os and
process/node:process/rustyjs:process. Defaults/CommonJS use their original objects;
the process object is also global. Named exports capture initial values/functions.

```js
import { sleep, setTimeout, clearTimeout } from 'rustyjs:timers';

const cancelled = setTimeout(() => console.log('cancelled'), 1000);
clearTimeout(cancelled);
await sleep(5);
console.log('done');
```

Run the example with `cargo run --locked -- examples/modules/builtins.mjs`.

## Export contract

| Named export | Same native function as | Behavior |
| --- | --- | --- |
| `sleep` | Initial `Rusty.sleep` | Promise fulfilling with undefined after a strict integer delay |
| `setTimeout` | Initial global `setTimeout` | One callback timer; returns a numeric ID |
| `clearTimeout` | Initial global `clearTimeout` | Clears either timer kind |
| `setInterval` | Initial global `setInterval` | Fixed-delay callback timer |
| `clearInterval` | Initial global `clearInterval` | Clears either timer kind |

All validation, argument forwarding, interval scheduling, rejection handling, and
cancellation rules are shared with the [existing API](api.md) and
[timer implementation](timers.md). There is no default export, Node-style Timeout
object, `ref`/`unref`, AbortSignal support, or `node:timers` alias.

Static imports, re-exports, and `import('rustyjs:timers')` work. Dynamic imports also
work in classic files and inline `<eval>` without a file base. Repeated/concurrent
imports share namespace identity within one runtime. Each dynamic call still
returns its own Promise.

Only explicitly registered specifiers/aliases are accepted. Unknown `rustyjs:*` names,
subpaths, different casing, query strings, and fragments fail without falling back
to filesystem or package lookup. Static failures produce module diagnostics;
dynamic failures reject with TypeError. Missing exports use V8's linking errors.
Import attributes and non-evaluation phases remain unsupported.

A package can map a private imports key such as `"#timers": "rustyjs:timers"` to
this module. That route shares the same cached namespace and function identities;
it does not create an additional built-in or a filesystem alias. See
[package maps](packages.md).

CommonJS `require('rustyjs:timers')` returns an ordinary cached object with the
same five native function objects. Its object identity is separate from the ESM
namespace; replacing a property on it does not replace the global function or
the ESM binding. Package-private aliases work in require mode too.

## Implementation walkthrough

1. Fresh-context setup installs native globals, then captures their original
   function handles before guest code runs. The module map owns these roots.
   Export evaluation never reads mutable guest globals or invokes their getters.
2. Resolution produces an explicit identity: a canonical file path or a registered
   built-in ID. Cache entries and graph edges use these identities, so built-ins
   cannot collide with filesystem paths.
3. The graph loader creates a V8 synthetic module with five declared export names.
   It caches that module alongside source modules. The usual linking callback
   resolves it, including validation of named imports and re-exports.
4. V8 invokes native evaluation steps on the isolate thread. Those steps set the
   exports from the captured handles and return a fulfilled Promise as required
   by the pinned V8 API. No JS source shim or filesystem read is involved.
5. Existing evaluation observation and dynamic-import settlement handle the module
   normally. Exported functions register work in the same host operation/timer
   registries as their global counterparts.
6. Cancellation releases outstanding work but preserves the module cache and its
   native export roots for context reuse. Runtime teardown releases both the
   synthetic module and captured function handles before disposing the isolate.

Replacing `globalThis.setTimeout` or `Rusty.sleep` does not change the exported
bindings. Initially they are identical function objects; later global property
assignments are independent. This is an API stability rule, not a sandbox or a
permission boundary. These functions still operate in their owning runtime.

Synthetic modules have no user source body, so this module has no observable
`import.meta` object. File modules keep their existing URL and entrypoint metadata.
CLI entrypoints remain filenames; `--module rustyjs:timers` is not an entry form.
The registry declares names per built-in identity and keeps captured values in
one map. Timers and module helpers use native functions and have no default
export. Events uses a captured JS constructor with default/named ESM exports and
a direct CommonJS constructor route. Buffer uses a captured JS constructor with
private Rust UTF-8 conversion callbacks. Only declared exports are available.

## Local verification

Tests cover static/dynamic namespace identity, re-exports, original function
identity, imported sleep/timeout/interval behavior, validation and cancellation,
inline imports, unknown names, missing exports, recovery after failed linking,
global getter replacement before the first import, and cached reuse after
cancelling a long sleep. These checks use the same local suite as file modules.
