# Phase 1: module system

## Current slice: file and built-in ES modules

Local-file ESM, dynamic imports, module metadata, and the first native built-in
are implemented alongside the classic-script runner. See the
[module walkthrough](modules.md), [built-in design](builtins.md), and
[supported API](api.md).

| Capability | Status |
| --- | --- |
| `.mjs` entrypoints and explicit `--module` / `-m` | Implemented |
| Relative, native absolute and file-URL imports | Implemented; paths are canonicalized and targets must exist |
| Module identity cache, live bindings, re-exports, cycles | Implemented through V8 with a host graph cache |
| Linking and file-specific errors | Implemented |
| Host-backed top-level await, including dependency evaluation | Implemented |
| Cached success/failure and preparation rollback | Implemented and tested |
| Dynamic `import()` in modules and classic scripts | Implemented with shared cache, concurrent evaluation, and catchable failures |
| `import.meta.url` and `import.meta.main` | Implemented with escaped canonical file URLs |
| `rustyjs:timers` built-in | Implemented with V8 synthetic modules and shared static/dynamic identity |
| Package resolution and package.json exports | Planned |
| CommonJS and mixed-module interoperability | Planned |

## Validation on 2026-11-18

Local platform: macOS arm64, Rust/Cargo 1.97.1, V8 152.2.0.

- `cargo test --locked`: 34 tests passed: 12 library, 15 general CLI, and 7 module
  CLI tests. All earlier Promise and timer tests are included.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo fmt --check`: passed.
- `cargo doc --locked --no-deps`: generated the Rust API reference successfully.
- `cargo run --locked -- examples/modules/main.mjs`: printed
  `ES modules + top-level await: 42` and exited normally.
- Markdown links and whitespace: valid across the nine project documents.

Compile/link retries are tested by creating a bad graph, verifying the newly added
cache entries are removed, repairing the source, and loading it successfully.
Cache semantics are tested by changing already-evaluated source and confirming it
does not re-execute. Evaluation failure and cancellation are tested separately.
Source fixes do not imply that V8 can restart an already-evaluated or cancelled
async module in the same runtime; that limitation is documented explicitly.

Linux and Windows behavior is not established by these local checks.

## Validation on 2026-11-19

Local platform remains macOS arm64, Rust/Cargo 1.97.1, V8 152.2.0. URL parsing and
file conversion use `url` 2.5.8 rather than custom escaping.

- `cargo test --locked`: 41 passed: 14 library, 15 general CLI, 12 module CLI.
- `cargo fmt --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo doc --locked --no-deps`: generated the Rust API reference successfully.
- `cargo run --locked -- examples/modules/dynamic.mjs`: prints answer 42, a true
  entrypoint flag, and its canonical escaped file URL.

New coverage includes dynamic/static namespace identity, concurrent pending
evaluation, nested top-level await, live bindings, classic imports, encoded
spaces/Unicode/hash names, file-URL imports from static and inline code, metadata,
catchable host errors, original SyntaxError and rejection values, failed-evaluation
caching, idle recovery through nested importers, and cancellation cleanup.

All validation runs locally. The GitHub workflow is manual-only and is not
triggered for this slice. No new cross-platform result is claimed.

## Built-in module validation on 2026-11-19

On the same local macOS arm64 toolchain:

- `cargo test --locked`: 46 passed: 15 library, 15 general CLI, 16 module CLI.
- `cargo fmt --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo doc --locked --no-deps`: generated the Rust API reference successfully.
- `cargo run --locked -- examples/modules/builtins.mjs`: prints
  `native module timers completed` and exits normally.
- Markdown links and whitespace: valid across ten project documents.

The new slice uses a file/built-in identity enum throughout the existing cache and
graph edges. It captures original native functions during context initialization
and exports them from a synthetic module without consulting guest globals at
evaluation time. Tests cover namespace/function identity, re-exports, imported
timer behavior, inline imports, invalid names, missing exports, failed-link
recovery, global property replacement, and cache reuse after cancellation.
GitHub Actions remains idle; no CI checks were triggered.

## Next slice: package-aware ESM resolution

Define a small package resolution contract, then add bare-package lookup through
node_modules and package.json entry/exports rules with explicit ESM format checks.
Keep CommonJS, mixed-module execution, HTTP imports, disk caches, and full Node
compatibility separate. An initial package loader must fail clearly when it meets
unsupported formats rather than treating CommonJS as working ESM.
