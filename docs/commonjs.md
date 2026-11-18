# CommonJS loader

## Contract

The runtime implements a bounded CommonJS loader and a default-only ESM bridge.
This does not imply full Node package compatibility.

- `.cjs` CLI entrypoints use CommonJS; `--commonjs file` explicitly selects it.
  Existing `.js` entrypoints remain classic scripts unless a mode flag is supplied.
  `--commonjs` respects formats: it cannot reinterpret `.mjs` or type-module `.js`.
- Each module gets function-local `exports`, `require`, `module`, `__filename`, and
  `__dirname`. The wrapper receiver is the initial exports object. Replacing
  `module.exports` changes subsequent requires; rebinding `exports` does not.
- Canonical filenames identify a per-runtime cache. A module enters the cache
  before its body runs, allowing circular dependencies to see partial exports.
  Failed modules are removed; successfully loaded dependencies stay cached.
- `require` is synchronous and module-local. Explicit relative/absolute filesystem
  paths support file and directory resolution, `.js`/`.json` extension probes,
  package main entries, and index.js/index.json defaults. `.cjs` is explicit.
  Native addons are unsupported. Filenames in require paths are literal.
  Bare names can also load single `.js` or `.json` files inside node_modules.
- Named packages and #imports reuse package resolution, patterns and arrays with
  `require`/`default` conditions. Selected exports/imports file targets stay exact;
  arrays never retry a missing selected target or a failed module body.
- `.cjs` and `.js` without type module execute as CommonJS. JSON can be required
  and uses the same cache. `require(ESM)` is unsupported, including synchronous
  ESM; use dynamic import. `require.resolve` uses the supported require resolver.
- ESM imports of CommonJS expose only a default export containing module.exports
  at evaluation time. No guessed named exports are promised. Object mutations are
  shared; later reassignment of module.exports does not update that ESM snapshot.
- Captured native timer functions are available through require('rustyjs:timers')
  and package aliases. The CommonJS object is cached separately from the ESM
  namespace while function objects are shared.
- Module metadata includes id, filename, loaded, exports, and require. Each bound
  require has main and resolve. require.cache, extension hooks, module.parent and
  module.children and broader Node globals/built-ins are separate work.
  EventEmitter is available through events/node:events; see [events](events.md).
  The first uncached explicit CommonJS entry is main (id `.`); cached modules do
  not get new wrapper metadata when a library caller enters them again. Modules
  imported only through ESM have no CommonJS main.
- Loading stays on the isolate thread. No V8 scope or Local handle crosses await.
  Synchronous load depth is capped at 64, including cached exports getters; V8 can
  throw a catchable stack RangeError earlier. This does not preempt synchronous JS.

Reference: [Node 24 CommonJS modules](https://nodejs.org/download/release/v24.11.0/docs/api/modules.html).
## Walkthrough

1. `execute_commonjs` canonicalizes the entry and enters the isolate/context with
   `TryCatch`. The CLI selects this path for `.cjs` or `--commonjs`, then runs the
   same microtask and Tokio event loop used by ESM and classic scripts.
2. The loader checks its depth guard, then reads the cache without holding a borrow
   across JavaScript. A cached module returns its current `module.exports`, which
   may be an accessor executing arbitrary guest code. The guard also covers this
   re-entry case.
3. A new module object starts with an empty exports object and loaded=false.
   Its persistent root is inserted before evaluation. A cleanup guard removes the
   entry if reading, compiling, calling, or finishing the module fails. Successful
   dependency modules remain cached when their parent fails.
4. `require` and `require.resolve` capture the canonical filename in V8 callback
   data. A later callback or a copied require function still resolves from its
   defining module, rather than the current host origin or process cwd. No global
   require is installed in classic scripts or ES modules.
5. V8 `compile_function` receives the original body and five argument names,
   avoiding source concatenation and wrapper-line offsets in diagnostics.
   Leading BOM and hashbang handling preserves subsequent line numbers. The
   initial exports object is both the exports argument and wrapper receiver.
6. JSON uses V8's native JSON parser, bypassing replaceable guest JSON globals.
   Syntax errors retain their SyntaxError type and include the JSON filename.
   JSON values use the same module cache as ordinary CommonJS exports.
7. After the body completes, loaded becomes true and the current exports value is
   returned. Metadata is written as own data properties so poisoned inherited
   setters cannot redirect host setup. Explicit readonly metadata changes produce
   a catchable error instead of a host panic.
8. ESM graph preparation creates a synthetic module declaring only `default` for
   each CommonJS target. During evaluation it calls the same loader, captures the
   resulting exports value, and settles the ordinary module evaluation Promise.
   Static and dynamic imports therefore share namespace identity, while require
   returns the underlying exports value.
9. Cancellation clears pending host operations and timers but keeps loaded module
   and builtin caches for context reuse. Runtime teardown clears those roots and
   removes the CommonJS isolate slot before disposing V8.

## Resolution and interoperability details

`import` activates import/default conditions; `require` activates require/default.
Their order still follows the manifest. Package-local aliases and external
dependency lookup use the same mode, including array and pattern selection.
See [package maps](packages.md).

Only legacy CommonJS file/directory lookup probes extensions and index files. A
missing legacy directory main may fall back to that directory's index. Export and
imports targets remain exact; once an array selects a target, a missing file,
unsupported format, or thrown body never selects the next array item.

`.mjs` is ESM, `.cjs` is CommonJS, and `.js` uses its nearest package type (untyped
means CommonJS for module loading). No syntax sniffing or guessed CommonJS named
exports are implemented. ESM JSON imports remain unsupported even though JSON can
be required. Explicit ES module entrypoints retain their source override, but a
file already loaded as CommonJS cannot be forced into ESM; requiring an explicitly
compiled ESM file is also rejected.

CommonJS body exceptions propagate the original thrown value. A caught require
failure can be retried, including after a source repair. An uncaught entry failure
cancels host work using the runtime's existing boundary policy; prior side effects
and successful dependency caches remain. The ESM bridge has its own evaluation
cache: a rejected bridge stays rejected even if a later require successfully
repairs and loads the CommonJS file. Use a fresh runtime to restart that ESM graph.

Replacing module.exports later affects subsequent require calls. An already
evaluated ESM bridge keeps the captured value. Mutations to the captured object
are visible to both, while reassignment creates a distinct value for require.
Native require objects are likewise ordinary mutable cached objects; modifying
their properties does not rewrite global functions or ESM native bindings.

Node-style error codes, require.cache mutation, custom extension hooks,
module.parent/children/paths, synchronous require(ESM), Node
other built-ins, and broader Node globals are not implemented yet. Host IO and source
compilation remain synchronous. There are no execution-time or memory limits.

## Require factories for ESM

```js
import { createRequire } from 'rustyjs:module';
const require = createRequire(import.meta.url);
const counter = require('./counter.js');
```

The native `rustyjs:module` built-in exposes one named export, `createRequire`.
CommonJS can obtain the same factory with `require('rustyjs:module')`. Each call
creates a distinct require function but shares the runtime's module/native caches.
No synthetic anchor module is loaded, no source at that filename is executed, and
the factory does not invent a require.main for an ESM entrypoint.

Only primitive absolute filename strings and file-URL strings are accepted.
Relative paths and other objects (including URL-like objects and boxed strings)
throw TypeError without invoking conversion hooks or href getters. The runtime
does not have a URL class yet; URL-object support is deferred to that surface.
URL queries, fragments, encoded separators, NUL, and non-local file URLs are
rejected. Native absolute path strings remain literal filesystem paths.

A base filename need not exist. The host normalizes its absolute components and
canonicalizes the nearest existing ancestor, retaining the virtual suffix. A
trailing native separator or file-URL slash means a directory base and adds an
internal filename anchor. The resulting path must be representable in UTF-8.
Neither normalization nor later resolution falls back to cwd.

Relative require paths are normalized before filesystem probing, so a virtual
base can use `../` to reach an existing module even when intermediate virtual
directories do not exist. Bare package and #imports lookup accepts the explicit
require anchor without requiring it to be a source file. ESM resolution still
requires its ordinary existing referrer file. Package conditions, format checks,
JSON parsing, require.resolve, error behavior, and require(ESM) rejection are
otherwise the same as module-local require.

The built-in registry now stores named native exports by built-in identity.
Synthetic ESM evaluation and CommonJS object creation read the same captured
function handles; mutating a CommonJS exports object cannot rewrite ESM bindings.
All roots are released with the module map before isolate teardown.

## Run and verify locally

```sh
cargo run --locked -- examples/commonjs/main.cjs
cargo run --locked -- examples/commonjs/esm.mjs
cargo run --locked -- examples/commonjs/factory.mjs
```

The first prints `CommonJS: 1` then `shared counter: 2`, proving require/import
share the same counter. The second starts a fresh runtime and prints
`ESM default: 1`.
The factory example prints `created require: 1` after comparing require and ESM
default identity.

Tests cover wrapper receiver/arguments/scope, metadata, exports rebinding and
replacement, cache identity, circular partial exports, JSON/probes/main/index
lookup, require/import conditions, native aliases, ESM snapshots and missing named
exports, original exceptions, failed-module retries, exact syntax locations,
cached-getter recursion, prototype setters, invalid arguments, ESM rejection,
bound require from later callbacks, BOM/hashbangs, cancellation/reuse, repaired
source, and incompatible mode re-entry.
Factory tests add ESM/CommonJS export identity, JSON and native cache reuse,
virtual native/file-URL bases, directory bases, package/private lookup, symlink and
encoded-name handling, scope lifetime after host awaits, unchanged main metadata,
and strict non-coercing validation.
