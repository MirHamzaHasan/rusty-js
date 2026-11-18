# Phase 2: minimum Node surface

Phase 2 is underway after the initial file/package/CommonJS module foundation.
Only the APIs listed as implemented below are available; this is not a full Node
compatibility target or a package/tooling replacement.

| Capability | Status |
| --- | --- |
| Core EventEmitter with ESM/CJS aliases | Implemented; see [events](events.md) for tested semantics and omissions |
| Buffer and binary encodings | Implemented subset; see [Buffer](buffer.md) for ownership, limits, and omissions |
| Readable/Writable streams | Planned after binary primitives and event behavior |
| path, os, process | Planned bounded surface |
| fs and crypto | Planned |
| fetch, Request/Response, URL | Planned |
| Express/package-suite compatibility | Future validation milestone when its required surface exists |

## Local validation on 2026-11-19

Platform: macOS arm64, Rust/Cargo 1.97.1, V8 152.2.0.

- `cargo test --locked`: 107 passed: 18 library, 12 Buffer CLI, 15 general CLI,
  14 CommonJS CLI, 11 events CLI, 16 module CLI, and 21 package CLI tests.
- `cargo fmt --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo doc --locked --no-deps`: generated the Rust API reference successfully.
- EventEmitter examples printed `event: 42` once and `CommonJS event: 42`.
- Buffer example printed `42 OK`, then `shared: NK copy: OK`.
- Markdown links and whitespace: valid across fifteen project documents.

Event tests exercise synchronous order and receivers, mutation snapshots,
duplicates, once reentrancy, listener metadata, symbols/property keys, errors,
Promise return behavior, advisory warnings, validation, and captured intrinsics.
Aliases node:events, events, and rustyjs:events share ESM namespace and constructor
identity. CommonJS gets the constructor directly. Buffer is also available as
described below; async events utilities and EventTarget remain separate work.

The implementation is bundled JavaScript executed by V8, with only warning IO
in Rust. No performance comparison is inferred from this feature. All validation
runs locally; the manual GitHub workflow stayed idle. Windows/Linux behavior is
not established by the local test results.

## Buffer slice

Buffer provides copies versus shared fixed backing stores, Uint8Array interop,
allocation/from/slice/subarray, UTF-8/hex/base64 and additional encoding rules,
byte operations, and fixed-width numeric IO. The 64 MiB per-Buffer guard is not a
runtime memory limit. Twelve Buffer tests cover the ownership/validation contract.
A locally run corpus matched Node v20.19.4 across 2,777 entries; this is correctness
evidence for those inputs rather than full compatibility or performance evidence.

## Next slice: bounded streams

Build Readable/Writable on the existing EventEmitter and Buffer APIs. Define
backpressure, event ordering, errors, destruction, and cancellation before wiring
filesystem or network handles. Do not expand into full Node compatibility.
Preserve the later AI, blockchain, dataframe, and GPU roadmap without claiming
those facilities are already shipped.
