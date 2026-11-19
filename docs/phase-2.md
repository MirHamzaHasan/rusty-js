# Phase 2: minimum Node surface

Phase 2 is underway after the initial file/package/CommonJS module foundation.
Only the APIs listed as implemented below are available; this is not a full Node
compatibility target or a package/tooling replacement.

| Capability | Status |
| --- | --- |
| Core EventEmitter with ESM/CJS aliases | Implemented; see [events](events.md) for tested semantics and omissions |
| Buffer and binary encodings | Implemented subset; see [Buffer](buffer.md) for ownership, limits, and omissions |
| Writable streams | Implemented subset; see [streams](streams.md) for backpressure, lifecycle, and scheduling |
| Readable and piping | Implemented subset; see [Readable](readable.md) for producer/consumer modes and cleanup |
| Async stream consumers and lifecycle helpers | Implemented subset; see [lifecycle helpers](stream-lifecycle.md) for cleanup and scoped pipeline behavior |
| path, os, process | Next bounded surface |
| fs and crypto | Planned |
| fetch, Request/Response, URL | Planned |
| Express/package-suite compatibility | Future validation milestone when its required surface exists |

## Local validation on 2026-11-19

Platform: macOS arm64, Rust/Cargo 1.97.1, V8 152.2.0.

- `cargo test --locked`: 168 passed: 18 library, 12 Buffer CLI, 15 general CLI,
  14 CommonJS CLI, 11 events CLI, 16 module CLI, 21 package CLI, 23 Readable CLI,
  16 Writable CLI, and 22 stream lifecycle CLI tests.
- `cargo fmt --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo doc --locked --no-deps`: generated the Rust API reference successfully.
- EventEmitter examples printed `event: 42` once and `CommonJS event: 42`.
- Buffer example printed `42 OK`, then `shared: NK copy: OK`.
- Writable example printed `hello world` after backpressure and asynchronous writes.
- Readable/pipe example printed `hello world` through an asynchronous sink.
- Async iteration example released its generator and reported closed after break.
- Supervised pipeline example printed `hello world` and `closed: true true`.
- Markdown links and whitespace: valid across eighteen project documents.

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

## Writable slice

Writable adds serialized byte/object-mode writes, advisory backpressure/drain,
corking, callback-based async hooks, finalization, and explicit destruction.
Notifications use Promise checkpoints rather than Node nextTick phases. Sixteen
integration tests cover queue/lifecycle ordering and failure cleanup. There is
no separate host scheduler, native IO adapter, or performance claim.

## Readable slice

Readable adds pull/push buffering, manual read sizes, flowing pause/resume,
multiple-destination backpressure, unpipe/EOF/error cleanup, and Readable.from
for sync/async iterable producers. Twenty-three tests cover behavior and reentrant
metadata failures. A selected 256-entry corpus matched local Node v20.19.4 for
buffered bytes, pressure, and asynchronous piping; tick-phase parity is not claimed.
Writable object mode now accepts undefined for compatible object pipelines.

## Async consumers and lifecycle helpers

Async iterators support FIFO next calls, early-return destruction, preservation
mode, and cleanup without close events. Callback and Promise finished/pipeline
helpers observe registered stream state, preserve the first failure, supervise
peer destruction, and clean up observers. Pipeline currently connects two endpoints;
transform/function stages remain separate work. Twenty-two lifecycle tests and
a selected 384-entry corpus matching Node v20.19.4 provide local correctness evidence.

## Next slice: path/os/process

Add bounded path utilities, host facts, and CLI process metadata, then move to
filesystem, crypto, and web APIs. Keep host IO permissions explicit for the later
AI sandbox phase; no security boundary exists yet.
Incremental text decoding remains explicit work. Do not expand into full Node compatibility.
Preserve the later AI, blockchain, dataframe, and GPU roadmap without claiming
those facilities are already shipped.
