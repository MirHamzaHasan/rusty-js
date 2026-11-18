# Phase 1: module system

## Current slice: files, built-ins, and installed ESM packages

Local-file ESM, dynamic imports, metadata, native timers, and a bounded installed
ESM package resolver are implemented alongside the classic-script runner. See the
[module walkthrough](modules.md), [built-in design](builtins.md), and
[supported API](api.md), and [package contract](packages.md).

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
| Scoped/unscoped node_modules lookup and self-reference | Implemented |
| Exact/pattern exports, ordered import/default conditions, main/index entries | Implemented subset; formats and target paths are validated |
| Fallback arrays and package-local #imports | Implemented with target/configuration error distinction and package scope boundaries |
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

## Package resolver validation on 2026-11-19

On macOS arm64 with Rust/Cargo 1.97.1 and V8 152.2.0:

- `cargo test --locked`: 58 passed: 16 library, 15 general CLI, 16 module CLI,
  and 11 package CLI tests. One symlink test is Unix-only.
- `cargo fmt --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo doc --locked --no-deps`: generated the Rust API reference successfully.
- `cargo run --locked -- examples/packages/main.mjs`: prints
  `package imports: 42 84` through package self-reference and host-backed await.
- Markdown links and whitespace: valid across eleven project documents.

Package tests build temporary dependency trees rather than committing installed
dependencies. They cover scoped/nested lookup, exports encapsulation, conditions
in manifest order, null blocks, main/index defaults, subpaths, self-reference,
static/dynamic/direct-file identity, encoded names, format boundaries, bad
metadata, unsupported exports, traversal, symlinks, and failure without parent
fallback. A library test repairs a bad manifest and retries in the same context.

Imports now require `.mjs` or `.js` with nearest `type: module`; unsupported and
CommonJS formats fail before evaluation. This deliberately replaces the earlier
all-extensions-as-ESM behavior. Explicit module entrypoints retain their source
override except for `.cjs`. CLI `.js` files still default to classic mode.

All results are local. GitHub Actions is manual-only and was not triggered.
These tests establish the documented RustyJS subset, not full Node compatibility.

## Package map validation on 2026-11-19

Local macOS arm64, Rust/Cargo 1.97.1, V8 152.2.0:

- `cargo test --locked`: 68 passed: 16 library, 15 general CLI, 16 module CLI,
  and 21 package CLI tests. One package symlink test is Unix-only.
- `cargo fmt --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo doc --locked --no-deps`: generated the Rust API reference successfully.
- `cargo run --locked -- examples/packages/maps.mjs`: prints `package maps: 42`.
- `cargo run --locked -- examples/packages/main.mjs`: still prints
  `package imports: 42 84`, now also exercising private imports and native aliases.
- Markdown links and whitespace: valid across eleven project documents.

New cases cover exact/prefix/trailer pattern priority, null exclusions, repeated
target substitution, arrays with invalid targets/null/unmatched branches, missing
files and unsupported formats without retry, non-index condition keys, template
and capture traversal, Unicode, combined depth limits, #imports namespace
identity, builtin and external aliases, nested scope boundaries, classic scripts,
inline rejection, unavailable selected targets, and unsupported alias cycles.

Direct #imports-to-#imports target chains remain intentionally unsupported; local
file cycles retain ordinary V8 graph semantics. Native aliases are RustyJS-specific
registered `rustyjs:*` names. The package contract records all retained omissions.
All validation is local; GitHub Actions stayed idle.

## Next slice: CommonJS execution and module boundaries

Design and implement a bounded CommonJS loader with module-local require,
module/exports wrapper semantics, circular-cache behavior, and useful filename
diagnostics. Define ESM/CommonJS boundaries explicitly before advertising package
compatibility. Keep package installation, HTTP imports, and full Node compatibility
outside the current scope.
