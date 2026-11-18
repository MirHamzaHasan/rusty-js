# Phase 2: minimum Node surface

Phase 2 is underway after the initial file/package/CommonJS module foundation.
Only the APIs listed as implemented below are available; this is not a full Node
compatibility target or a package/tooling replacement.

| Capability | Status |
| --- | --- |
| Core EventEmitter with ESM/CJS aliases | Implemented; see [events](events.md) for tested semantics and omissions |
| Buffer and binary encodings | Next slice |
| Readable/Writable streams | Planned after binary primitives and event behavior |
| path, os, process | Planned bounded surface |
| fs and crypto | Planned |
| fetch, Request/Response, URL | Planned |
| Express/package-suite compatibility | Future validation milestone when its required surface exists |

## Local validation on 2026-11-19

Platform: macOS arm64, Rust/Cargo 1.97.1, V8 152.2.0.

- `cargo test --locked`: 95 passed: 18 library, 15 general CLI, 14 CommonJS CLI,
  11 events CLI, 16 module CLI, and 21 package CLI tests.
- `cargo fmt --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo doc --locked --no-deps`: generated the Rust API reference successfully.
- EventEmitter examples printed `event: 42` once and `CommonJS event: 42`.
- Markdown links and whitespace: valid across fourteen project documents.

Event tests exercise synchronous order and receivers, mutation snapshots,
duplicates, once reentrancy, listener metadata, symbols/property keys, errors,
Promise return behavior, advisory warnings, validation, and captured intrinsics.
Aliases node:events, events, and rustyjs:events share ESM namespace and constructor
identity. CommonJS gets the constructor directly. Other Node built-ins remain
unimplemented; async events utilities and EventTarget are separate work.

The implementation is bundled JavaScript executed by V8, with only warning IO
in Rust. No performance comparison is inferred from this feature. All validation
runs locally; the manual GitHub workflow stayed idle. Windows/Linux behavior is
not established by the local test results.

## Next slice: Buffer and encoding contract

Start with byte ownership, Uint8Array interop, allocation/from/slice/subarray, and
UTF-8/hex/base64 encoding rules. Define copies versus shared backing stores and
verify malformed inputs and bounds before adding streams or filesystem APIs.
Preserve the later AI, blockchain, dataframe, and GPU roadmap without claiming
those facilities are already shipped.
