# RustyJS agent instructions

## Owner directions

On 2026-11-17 Hamza explicitly removed the human-written-only restriction and
authorized implementation: "leave that rule of human writtne thing start the
development". Implement, debug, and test the project autonomously. The earlier
mentoring-only restriction no longer applies. Continue explaining design choices
and reviewing correctness honestly.

Explain new tasks piece by piece and explain why each part exists. Be direct and
brief, assume senior Rust/TypeScript experience, and recommend one concrete next
step suitable for roughly one hour per day. Prefer free, open-source tooling.

Do not run the GitHub CLI (`gh`). Prefer the GitHub integration, then the browser
for unsupported administration. Avoid routine yes/no confirmation questions;
proceed within the user's authorization. Required tool/platform approvals still
apply. Use feature branches and pull requests; do not push directly to `main`,
force-push it, or weaken its protection.

The active owner goal explicitly requires continuing feature implementation,
updating documentation, running real tests, and committing/pushing tested work to
GitHub incrementally. Use Git and the GitHub integration; never invoke `gh`,
including through a Git credential helper. Preserve the full product roadmap.

On 2026-11-19 the owner directed local testing only and no GitHub CI for now.
Do not trigger GitHub Actions or add automatic CI triggers. The workflow is
manual-only; leave it idle unless the owner explicitly asks for remote checks.
Run formatting, Clippy, tests, and documentation checks locally before publishing.

## Product and scope

Rust host, V8 guest through rusty_v8, and Tokio driving async work. This is a
runtime embedding an existing engine. The intended positioning is "Rust performance
without leaving JavaScript," with future native facilities for AI sandboxes,
blockchain, data science, and GPU compute. These are plans, not shipped features.

Follow this sequence: Phase 0 core engine; Phase 1 modules; Phase 2 minimum Node
surface; Phase 3 AI; Phase 4 blockchain; Phase 5 data science; Phase 6 GPU.
Full Node compatibility, package-manager/tooling competition, and an application
framework are outside the current scope. Push back on scope creep.

## Engineering review constraints

- Keep the V8 isolate on its owning thread. Prefer a current-thread Tokio runtime.
  Ordinary `tokio::spawn` still requires `Send` on that runtime. Add a `LocalSet`
  only when local spawned tasks are needed; never send V8 handles to workers.
- Treat Tokio progress and V8 microtasks as separate responsibilities. Synchronous
  JS blocks the owning thread; Tokio alone does not preempt it.
- For the later async Promise bridge, retain the resolver using a persistent V8
  handle on the isolate thread. Workers return owned Rust results through a queue;
  the runtime re-enters V8 to settle Promises and pump microtasks. Review handle
  cleanup, cancellation, task errors, and shutdown explicitly.
- Inspect the pinned V8 API before recommending signatures. Many APIs return
  `Option`; convert absence into useful errors where appropriate. Scope-bound
  handles must not survive their scopes or be held across `.await`.
- Catch ordinary JS compile/runtime exceptions at the execution boundary; malformed
  guest code must not cause a host panic. A CLI can report failure with a nonzero
  exit status. Do not claim recovery from fatal V8 failures or enforced resource
  limits until verified.
- Cross-platform V8 builds and Windows/macOS/Linux CI are explicit milestones.
  Distinguish a successful local build from tested platform support.

## Claims and evidence

Raw JS execution uses V8 and its JIT; do not claim it is inherently faster than
Node's. Benchmark locally before publishing speedups. Record versions, workload,
hardware, concurrency, correctness, startup costs, latency distributions, and
reproduction commands. A target is not a result.

Any performance hypothesis must identify a measurable mechanism: avoiding GC in
Rust-owned work, parallel native work, scheduler/IO costs, parsing before JS object
creation, reduced boundary conversion, or smaller startup work. Tokio superiority,
zero-copy transfer, and zero-cost bindings are not automatic guarantees.

Compare Python integrations over the complete round trip; Polars in Python already
uses Rust. Measure Node compatibility using real package test suites, starting with
Express when the required runtime surface exists. Keep every public claim defensible.

## Repository status

The checkout started empty on 2026-11-17. After Hamza authorized implementation,
the Phase 0 baseline was created on `feat/phase-0-runtime`: CLI inline/file execution,
current-thread Tokio, native logging, reusable contexts, and exception diagnostics.
V8 is pinned to 152.2.0. Keep `Cargo.lock` tracked. See `docs/phase-0.md` for
validation evidence and the next task.

On 2026-11-18 the async slice added explicit microtask checkpoints, `Rusty.sleep`,
a Tokio Promise bridge, checkpoint rejection detection, and cancellation cleanup.
Keep the API reference (`docs/api.md`) and async walkthrough (`docs/async-runtime.md`)
current alongside implementation changes.

The callback timer slice adds setTimeout/clearTimeout and setInterval/clearInterval,
strict callback/delay validation, fixed-delay intervals, and cancellation cleanup.
Keep `docs/timers.md` current.

Phase 1 now supports local-file static and dynamic ES module graphs, canonical
identity, caching, linking/evaluation, cycles and host-backed top-level await.
Dynamic failures reject catchable Promises; import.meta exposes canonical file
URLs and an entrypoint flag. Keep `docs/modules.md` and `docs/phase-1.md` current.
The `rustyjs:timers` synthetic module exports the original native sleep and callback
timer functions through the same static/dynamic cache. Keep `docs/builtins.md`
current. Package-aware ESM now handles node_modules, scoped names, self-reference,
exact/pattern exports, fallback arrays, ordered import/default conditions,
package-local #imports, and explicit main entries.
Imported .js uses package.json type: module means ESM, otherwise CommonJS; .cjs
uses CommonJS. Keep
`docs/packages.md` current. Arrays skip invalid target shapes, not missing selected
files or dependencies. #imports aliases may target local files, packages, and
native built-ins, but not another #alias.

CommonJS now supports .cjs / --commonjs entrypoints, module-local require and
metadata, canonical caches/cycles/retries, JSON, legacy file/directory probes,
require/default conditions, and a default-only ESM bridge using the same cache.
require(ESM) remains explicitly rejected. Never hold registry borrows across guest
calls or exports getters; the synchronous load guard includes cached accessors.
Keep `docs/commonjs.md` current. rustyjs:module now exports createRequire for
explicit absolute filename/file-URL string anchors, including virtual filenames.
Its bound functions share the CommonJS/native caches without creating a main.
URL objects and loader hooks remain unsupported. Next start the Phase 2 minimum
Node surface with EventEmitter.
No package manager or full Node compatibility is implied.

GitHub ruleset `27446578` was created and verified active on 2026-11-17, targeting
`refs/heads/main`: pull requests required, force pushes and deletion blocked, no
bypass actors, and zero required approving reviews for solo development. The repo
had no remote branch at setup, so GitHub reported zero matching targets. Preserve these
protections during initialization; do not silently disable them to seed the repo.
