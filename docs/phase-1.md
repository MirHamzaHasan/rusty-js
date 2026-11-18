# Phase 1: module system

## Current slice: static local ES modules

The local-file ESM slice is implemented alongside the classic-script runner.
See the [module walkthrough](modules.md) and [supported API](api.md).

| Capability | Status |
| --- | --- |
| `.mjs` entrypoints and explicit `--module` / `-m` | Implemented |
| Relative and native absolute local-file imports | Implemented; paths are canonicalized and targets must exist |
| Module identity cache, live bindings, re-exports, cycles | Implemented through V8 with a host graph cache |
| Linking and file-specific errors | Implemented |
| Host-backed top-level await, including dependency evaluation | Implemented |
| Cached success/failure and preparation rollback | Implemented and tested |
| Dynamic `import()` and `import.meta` host metadata | Next slice |
| `rustyjs:*` built-ins, package resolution and exports | Planned |
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

## Next slice: dynamic imports and module metadata

Add `import()` using the same resolver/cache, then populate `import.meta.url` and
the entrypoint marker. First decide file-URL conversion and resolution semantics,
including escaping and Windows drive paths; do not hand-roll URL encoding.

Verify repeated and concurrent dynamic imports share identity, rejected imports
remain catchable Promises, top-level await completes through the host loop, and
`import.meta` points at the defining module rather than the current process cwd.
Keep package lookup, CommonJS, network imports, and disk code caches separate.
