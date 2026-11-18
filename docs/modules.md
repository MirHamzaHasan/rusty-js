# Local-file ES modules

Phase 1 loads static and dynamic local module graphs and evaluates them in
the runtime's existing context. V8 supplies JavaScript module semantics; RustyJS
supplies file resolution, graph preparation, caching, and event-loop integration.

```sh
cargo run --locked -- examples/modules/main.mjs
cargo run --locked -- --module tests/fixtures/modules/as-js.js
```

## Entry and resolution contract

- `.mjs` CLI entrypoints use module mode automatically. `--module file` or `-m file`
  selects it explicitly for another extension. Other file entrypoints and `-e`
  retain classic-script behavior.
- Specifiers must start with `./` or `../`, be native OS absolute paths, or use
  `file:` URLs. Relative imports resolve against the importing file's canonical
  URL, independent of the process working directory. The `url` crate performs URL
  conversion; relative references are percent decoded. Native absolute paths stay
  literal filesystem paths.
- The target must be an existing regular file containing UTF-8 JavaScript module
  source. URL queries/fragments and escaped path separators are rejected rather
  than assigned separate cache identities. Extension guessing, directory indexes,
  and HTTP loading are absent. A file without an extension works if its exact name
  is requested. For example, `./space%20%23%20caf%C3%A9.mjs` loads `space # café.mjs`.
- Imported files are parsed as ESM regardless of their extension. There is no
  package.json format inference, JSON loader, bare-package resolution, CommonJS,
  import attributes, or source/deferred import phases in this slice.
- `import()` also works in classic files. Inline `<eval>` has no relative file
  base; absolute paths and file URLs still work. A library script origin used as a
  relative base must name an existing file.
- `import.meta.url` is the canonical escaped file URL. `import.meta.main` marks
  the first explicit ESM entrypoint; dependencies are false. Metadata is created
  once, so loading a module dynamically before a later explicit entry call does
  not update its already-created metadata. `import.meta.resolve` is absent.

## Walkthrough

1. `execute_module` canonicalizes the entry path and enters the runtime's context
   with a V8 `TryCatch`. Canonical paths are module identity; symlink aliases and
   equivalent path spellings share one cached module.
2. `load_graph` uses a Rust worklist rather than recursive file-loading calls.
   Each uncached file is read and compiled with a module `ScriptOrigin` containing
   its canonical filename. Files are currently read synchronously.
3. The compiled `Global<Module>` is cached before its imports are visited. A cycle
   can therefore refer back to the same module instead of causing repeated loads
   or unbounded recursive Rust calls.
4. V8's module requests supply specifiers and source offsets. Rust validates each
   request, resolves the target file, and records a `(referrer, specifier)` edge.
   Unsupported or missing imports get a referrer filename and source location.
5. After the graph is prepared, V8 instantiates it. The resolver callback only
   finds precompiled dependency handles; it performs no filesystem reads or new
   compilation. Referrers are compared by object identity, not V8's non-unique
   identity hash. The initial reverse lookup is linear in cached module count.
6. V8 links bindings and evaluates modules in dependency order. Defaults, named
   exports, re-exports, live bindings, and valid cyclic graphs use V8's native
   implementation rather than a Rust imitation of those language rules.
7. The returned evaluation Promise is retained in the cache and explicitly marked
   handled. RustyJS observes that Promise directly, so a rejected entry evaluation
   is not also reported as a duplicate generic unhandled rejection.
8. The runtime checkpoints microtasks and inspects active entry evaluations.
   A fulfilled entry is no longer active. A rejected entry returns its module
   diagnostic; any other unhandled guest Promise still follows the existing
   rejection policy.
9. Pending top-level await is driven by the existing Tokio loop. Both
   `await Rusty.sleep(ms)` and awaiting a Promise resolved by a callback timer work,
   including through imported dependencies.
10. Once host operations and timers are idle, an entry still awaiting a Promise
    becomes a controlled error: `module evaluation is pending with no active host
    work`. It is not silent success and does not busy-spin forever.

## Dynamic import walkthrough

1. V8's dynamic import callback creates a Promise resolver and queues the requested
   specifier and defining script filename. It performs no file IO or nested graph
   evaluation inside the callback. V8 152 invokes the phase-aware callback for
   evaluation imports too; both callback forms route to the same queue.
2. After a microtask checkpoint, the host drains that queue on the isolate thread.
   It uses the same resolution, graph loader, and evaluation cache as static
   entrypoints. Repeated/concurrent calls have separate Promises but share module
   identity, evaluation, live bindings, and the final namespace object.
3. Each pending import retains its own `Global<PromiseResolver>` and target path.
   Evaluation Promises are host-observed and marked handled; the returned import
   Promise belongs to guest code and follows ordinary rejection tracking.
4. Fulfilled evaluation resolves the caller's Promise with the module namespace.
   Rejected evaluation forwards the original thrown value. Compile/link failures
   preserve V8 exception types and values; host resolution/read failures use
   TypeError. A `.catch()` or `try/await/catch` handles these failures without
   failing an unrelated entry evaluation or cancelling its host work.
5. Settlement is followed by another microtask checkpoint. Reactions can request
   further imports, sleeps, or timers; the host rechecks work before awaiting or
   declaring idle. No V8 handle or scope crosses an async suspension.
6. At true host idle, the newest blocked import rejects with TypeError, then the
   host checkpoints again before considering another blocked import. An inner
   catch can therefore recover its module before a waiting importer is rejected.
   Guest code can catch this runtime-specific idle error. A root still pending
   afterward receives the entry idle diagnostic.
   Cyclic top-level await that depends on its own completed evaluation cannot make
   progress; ordinary static cycles without such an await remain supported.

## Cache and failure lifetime

The cache belongs to one `JsRuntime` and retains compiled modules and entry
evaluation Promises until that runtime is dropped. This is an in-memory identity
cache, not V8 serialized code caching or a persistent disk cache. There is no
eviction or cache-size budget yet.

Changing a successfully cached source file does not reload it. Re-entering that
module returns the same evaluated module, and a cached evaluation failure remains
a failure. A fresh runtime provides a fresh module graph and context.

Preparation is transactional for newly cached entries: a file read, compile, or
link failure removes the new graph entries and their edges. Fixing the source and
retrying can then work. Previously cached modules are preserved. This rollback
happens before evaluation; it does not undo JavaScript side effects.

Cancelling host work stops actively watching module evaluation and clears host
resolver/timer roots plus queued and pending dynamic import resolvers. Their
caller Promises remain pending. Module-cache handles remain intentional ownership.
A cancelled module awaiting a cancelled host Promise may remain pending in V8;
re-entering it without new host work reports the idle-await error. Use a fresh
runtime to restart that graph. Other scripts can still use the existing context.

Runtime teardown clears module handles and removes the module-map isolate slot
before disposing the isolate. No scope-bound module handle or open V8 scope crosses
an async wait, and no module handle is sent to a Tokio worker. Persistent globals
remain on the owner thread.

## Verified behavior and limits

CLI tests run from a temporary working directory while using absolute entry paths,
so correct relative resolution cannot accidentally depend on the repository cwd.
They cover shared graph identity, canonical path aliases, live bindings,
re-exports, cycles, transitive top-level await, explicit `.js` module mode, load and
link errors, rejected evaluation, and idle unresolved await.
Dynamic checks cover concurrent evaluation, namespace identity shared with static
imports, live bindings, nested host-backed await, classic-script imports, original
rejection identity, catchable compile/link/load failures, escaped names, file URLs,
metadata, and caught idle imports.

Library tests cover source changes after caching, symlink aliases on Unix,
cached failures, failed-graph rollback and retry, cancellation, context reuse,
dynamic waiter cleanup, and file URLs in static and inline imports.
The ordinary script, Promise, and callback-timer suites remain in the same run.

This is a trusted local-file loader. It does not impose a filesystem sandbox,
graph-size limit, code-execution timeout, or memory cap. Checks currently run
locally; the optional GitHub workflow is manual-only and stays idle. Local macOS
results alone do not establish Windows or Linux support.
