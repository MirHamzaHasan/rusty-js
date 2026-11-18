# Buffer: bytes, ownership, and encodings

Buffer is a bounded subset of the Node byte API, built on V8's Uint8Array storage.
It is available globally and as the named `Buffer` export from `rustyjs:buffer`,
`node:buffer`, and `buffer`. All three aliases share one ESM namespace. CommonJS
gets a cached object with a `.Buffer` property referencing that same constructor.
There is currently no default ESM export or other buffer-module exports.

```js
import { Buffer } from 'node:buffer';

const packet = Buffer.alloc(8);
packet.writeUInt32BE(42, 0);
packet.write('OK', 4);
const payload = packet.subarray(4, 6);
console.log(packet.readUInt32BE(0), payload.toString()); // 42 OK
```

Run `cargo run --locked -- examples/buffer/main.mjs`.

## Copies and shared storage

| Operation | Ownership |
| --- | --- |
| `Buffer.alloc(size[, fill[, encoding]])` | New zeroed storage, optionally filled |
| `Buffer.allocUnsafe(size)`, `allocUnsafeSlow(size)` | Also new zeroed storage; no slab pool or uninitialized bytes |
| `Buffer.from(string[, encoding])` | Newly encoded bytes |
| `Buffer.from(bufferOrTypedArray)` | Copies element values into bytes; Uint16Array elements are truncated, not reinterpreted |
| `Buffer.from(arrayLike)` | Copies numeric elements using Uint8Array assignment conversion |
| `Buffer.from({type: 'Buffer', data: [...]})` | Copies the JSON representation |
| `Buffer.from(arrayBuffer[, byteOffset[, length]])` | Shares the specified fixed ArrayBuffer region |
| `buffer.slice(start, end)`, `subarray(start, end)` | Shared Buffer views, including negative/clamped indices |
| `Buffer.concat(list[, totalLength])` | Copies into new storage; truncates or zero-pads to the requested length |

Buffers are real Uint8Arrays: indexing, `.buffer`, `.byteOffset`, iteration, and
ordinary typed-array methods work. Inherited allocation methods such as `map`
return Uint8Arrays through the species setting. Use `Buffer.from(view)` when an
independent Buffer copy is needed. Buffer.slice deliberately shares storage.
`Buffer.isBuffer(value)` uses an internal brand; changing a plain Uint8Array's
prototype does not make it a Buffer.

One Buffer allocation/view is limited to **64 MiB** in this initial implementation.
Sizes, byte offsets, counts, and numeric operation offsets must be primitive
nonnegative integer Numbers in range. Fractional sizes are rejected. This limit
is an API guard, not an isolate memory budget: multiple Buffers, ordinary V8
arrays, and intermediate strings can allocate more memory. Fatal V8/host allocation
failures are not recoverable exceptions promised by this API.

Resizable and SharedArrayBuffer inputs to Buffer.from are rejected. Typed-array
inputs over them may be copied into ordinary fixed storage. DataView is not a
Buffer.from array-like source. Detached Buffer operations that access storage
throw TypeError before native UTF-8 conversion. There is no transfer API supplied
by Buffer itself. Deprecated `new Buffer(...)`, valueOf/Symbol.toPrimitive input
coercion, slab pooling, and subclass construction are outside this slice.

## Encodings

Names are case-insensitive. Omitted or empty encoding defaults to UTF-8.
Unknown/non-string encodings throw TypeError. `Buffer.isEncoding('')` returns false.

| Encoding | Behavior |
| --- | --- |
| utf8 / utf-8 | Replaces lone UTF-16 surrogates when encoding and malformed UTF-8 when decoding with U+FFFD |
| utf16le / utf-16le / ucs2 / ucs-2 | Preserves UTF-16 code units; decoding ignores a final odd byte |
| latin1 / binary | Keeps the low eight bits of each code unit |
| ascii | Encoding keeps eight bits; decoding clears each byte's high bit |
| hex | Decoding stops at the first invalid pair and ignores a final unpaired nibble; output is lowercase |
| base64 | Accepts standard and URL-safe alphabets, ignores nonalphabet characters, stops at the first equals sign; output includes padding |
| base64url | Same permissive decoder; output uses URL-safe alphabet without padding |

`toString(encoding, start, end)` decodes only the requested clamped byte range.
It does not repair a slice cut through a UTF-8 codepoint: replacement characters
are expected. `Buffer.byteLength(string, encoding)` counts actual decoded bytes,
including for malformed hex/base64. This differs from Node's optimistic length
estimates for some malformed inputs. Buffers, ArrayBuffers, and ArrayBuffer views
report their intrinsic byte length without interpreting an encoding argument.

`write(string[, offset[, length]][, encoding])` returns the bytes written. It
clamps length to remaining capacity and never writes a partial UTF-8 codepoint or
UTF-16 code unit. It currently encodes the full input before copying, so the
64 MiB encoded-input limit applies even when the destination is smaller.

## Byte operations and numeric IO

- `copy(target[, targetStart[, sourceStart[, sourceEnd]]])` accepts Buffer or
  Uint8Array, supports overlapping regions, and returns the number copied.
- `fill(value[, offset[, end]][, encoding])` accepts a number, string, Buffer, or
  Uint8Array. It snapshots an aliased byte pattern before writing. An empty string
  zero-fills; another empty or undecodable pattern throws TypeError.
- `equals(other)`, `compare(other)`, and `Buffer.compare(a, b)` compare unsigned
  bytes lexicographically. The instance compare method currently accepts only
  the whole target, without Node's optional range arguments.
- `toJSON()` produces `{type: 'Buffer', data: [...]}`.

Fixed-width `read`/`write` methods cover UInt8/Int8, UInt16/Int16, UInt32/Int32,
Float, Double, BigUInt64/BigInt64, with LE/BE suffixes for multibyte types.
The Uint/BigUint spelling aliases are included. Offset defaults to zero; writes
return the first offset after the written value. Integer values must fit exactly;
64-bit values require BigInt. Float/Double accept Number, including NaN/Infinity.
Numeric strings and fractional integer values are explicitly rejected rather
than coerced. Every operation bounds-checks the entire width before mutation.

Variable-width integer methods, search/includes/indexOf, swaps, compare ranges,
copyBytesFrom, Blob/File, transcode, atob/btoa, inspection hooks, and Node error
codes remain unimplemented. Express's test suite is still a future milestone;
this subset does not establish package compatibility.

## Implementation walkthrough

1. Fresh-context setup compiles the bundled JS function body before guest code.
   It captures byte/encoding intrinsics and receives three private Rust callbacks.
   The returned constructor is installed globally and rooted by the built-in map.
2. Allocations and views use V8's native Uint8Array/ArrayBuffer objects. A WeakSet
   holds brands without retaining dead Buffers; no Rust registry holds each byte
   object. Numeric IO uses captured DataView operations on the current region.
3. UTF-8 encoding checks the pinned V8 string's encoded length, writes with
   `kReplaceInvalidUtf8`, then gives owned bytes to V8 through the safe
   `new_backing_store_from_bytes` API. No raw pointer or borrowed slice escapes.
4. Decoding copies a validated Uint8Array region through `copy_contents` and uses
   Rust's lossy UTF-8 decoder. It then creates a V8 string in the same scope.
   These conversions copy; there is no zero-copy or zero-cost claim.
5. All callbacks are synchronous on the isolate thread, with no handle crossing
   an await or worker boundary. Module capture, cache identity, and teardown use
   the existing [built-in lifetime rules](builtins.md).

The Node API reference used for the selected surface is the
[Node v24.11.0 Buffer documentation](https://nodejs.org/download/release/v24.11.0/docs/api/buffer.html).
RustyJS implements its own subset; the reference does not imply complete parity.

## Local correctness evidence

On macOS arm64 with Rust/Cargo 1.97.1 and V8 152.2.0, the Buffer integration tests
cover aliases/global identity, copying versus shared views, malformed encodings,
overlap, fill patterns, write truncation, numeric IO, validation, detached storage,
and captured intrinsics. The shared corpus below also produced **2,777 identical
entries** with locally installed Node v20.19.4. This is correctness evidence for
those inputs, not a benchmark or a full compatibility score.

```sh
cargo build --locked
node tests/fixtures/buffer-conformance.js > /tmp/rustyjs-buffer-node.json
target/debug/rustyjs tests/fixtures/buffer-conformance.js > /tmp/rustyjs-buffer-runtime.json
cmp /tmp/rustyjs-buffer-node.json /tmp/rustyjs-buffer-runtime.json
cargo test --locked --test buffer
```

Other platforms have not been validated. No GitHub CI is required or triggered.
