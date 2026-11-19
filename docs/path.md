# Lexical path utilities

The path module supplies POSIX and Windows string operations without opening
files. `path`, `node:path`, and `rustyjs:path` share one ESM namespace and use the
host platform's variant. Explicit posix/win32 variants are available on every host.

```js
import path, { posix, win32 } from 'node:path';

console.log(posix.normalize('src/../app/main.js')); // app/main.js
console.log(win32.join('C:\\app', '..', 'data', 'rows.csv')); // C:\data\rows.csv
console.log(path.resolve('data')); // anchored to the host cwd
```

Run `cargo run --locked -- examples/path/main.mjs`. No global path object is
installed. os/process modules are subsequent work, not implied by cwd-backed
path resolution.

## Exports and identity

| Module aliases | Default/CommonJS object |
| --- | --- |
| path / node:path / rustyjs:path | Host variant: posix on Unix, win32 on Windows |
| path/posix / node:path/posix / rustyjs:path/posix | posix object |
| path/win32 / node:path/win32 / rustyjs:path/win32 | win32 object |

Each namespace exposes default plus named resolve, normalize, isAbsolute, join,
relative, toNamespacedPath, dirname, basename, extname, format, parse, sep,
delimiter, posix, and win32. The explicit variants have distinct module identities
but share their underlying objects/functions with the main module. On Unix,
`require('path') === require('path/posix')`; on Windows the corresponding identity
is with path/win32. Every variant's posix/win32 properties point to the same pair.

CommonJS returns the variant object directly, without a synthetic default property.
These objects are ordinary mutable objects. ESM function bindings capture their
original values before guest code, so replacing a CommonJS property does not
replace a named ESM function. A default or posix/win32 object reference still sees
property mutations normally. Built-in aliases take priority over installed packages.

## Normalization, joining, and resolution

- `normalize(path)` collapses repeated separators and dot segments, retains a
  meaningful trailing separator, and returns `.` for empty relative input. Above-root
  parent segments are discarded for absolute paths and retained for relative ones.
- `join(...paths)` ignores empty strings and normalizes the concatenation. A later
  absolute-looking component does not reset earlier components. Empty input gives `.`.
- `resolve(...paths)` processes components from right to left, returning an absolute
  lexical path. Relative input uses the current host working directory when needed.
  No arguments resolves cwd; explicit absolute anchors can avoid reading cwd.
- `relative(from, to)` resolves its inputs then produces a relative traversal, or
  an absolute Windows destination when the roots have no common component. Equivalent
  paths return an empty string. Windows comparisons are case-insensitive while output
  retains target spelling.
- `isAbsolute(path)` recognizes syntax, rather than checking existence or containment.

POSIX uses `/` as the separator and `:` as the list delimiter. Backslashes are
ordinary filename characters. normalize/resolve collapse multiple leading slashes;
dirname/parse retain raw separator details according to their component rules.

Windows accepts slash and backslash, emits backslashes for normalized results,
and uses `;` as the list delimiter. It recognizes drive-relative `C:child`,
drive-absolute `C:\child`, rooted `\child`, and UNC server/share roots. A leading
UNC root is preserved by join only when the first nonempty input expresses that
intent; joining `/` with `/server` does not invent a UNC share. Relative normalize
results that could become drive syntax after dot removal receive a `.\` prefix.

Drive-relative resolution uses the current host cwd. A same-drive or drive-less
cwd contributes its tail; an incompatible drive falls back to that drive's root.
Windows per-drive environment cwd entries are not implemented. On Windows, the
POSIX variant converts cwd separators and omits its drive prefix. Windows-native
behavior has not been validated by the local macOS build.

`toNamespacedPath(path)` is a no-op in POSIX. For Windows it resolves string input,
converts drive/UNC paths to the `\\?\` namespace when applicable, and preserves
existing device namespace forms. Empty/non-string inputs pass through unchanged;
very short resolved roots retain the original input. This is lexical conversion,
not reserved-device validation or a promise that Windows will accept the filename.

## Components and formatting

`dirname`, `basename`, and `extname` inspect the original string without resolving
or normalizing it. basename optionally removes an exact, case-sensitive suffix,
including on Windows. extname returns the final dot suffix; dotfiles without a
later dot and the component `..` have no extension, while `file.` ends in `.`.

`parse(path)` returns root, dir, base, ext, and name as own data properties. It
retains raw separators and recognizes a complete UNC share as a root. Standalone
basename/extname and parse can consequently differ on a UNC root. The selected
Node-compatible parse behavior also includes the rooted POSIX `/..` extension
quirk; do not infer extname solely from parse for unusual inputs.

`format(object)` gives dir priority over root and base priority over name/ext. It
adds a dot to nonempty ext when omitted. If dir equals root, no extra separator
is inserted; otherwise one is inserted literally, even if dir already ends in
a separator. Formatting does not normalize the result.

Path arguments and suffixes must be primitive strings; objects are not coerced.
All resolve/join arguments are validated even when a later absolute path would
make an earlier argument unnecessary. format requires a non-null, non-array object;
every present component field must be a string, including fields later overridden
by priority. These stricter validation rules are deliberate. Property getters are
ordinary guest code: their thrown values propagate. Internal formatting storage
ignores inherited setters.

There is no glob matching, file-URL conversion, filesystem canonicalization,
drive-state management, chdir, or deprecated `_makeLong` alias in this slice.
Path strings can contain values the host filesystem rejects. This API neither
checks permissions nor prevents traversal or symlink escapes. Module loading keeps
its existing canonical referrer-based rules and does not delegate to this utility.

## Host failures and implementation walkthrough

1. Fresh-context setup compiles a bundled JS factory with a private native cwd
   callback and a host-platform flag. It builds both variants before guest code.
   Captured string/array intrinsics provide selected API stability rather than a
   hardened realm.
2. Root scanning separates drive/UNC roots from lexical components. Dot reduction
   operates on components, while parsing retains original spans and separators.
   Both variants run in V8; there is no Rust filesystem parser on these paths.
3. resolve calls cwd only if its arguments do not supply a sufficient anchor.
   The synchronous callback uses `std::env::current_dir` on the isolate thread.
   It converts host/encoding failures into JS errors and holds no V8 handle across
   await. Paths are never silently replaced using lossy UTF-8 conversion.
4. The synthetic built-in registry captures objects and function values. All nine
   aliases resolve before filesystem/package fallback. Unknown subpaths and URL
   suffixes are rejected by the ordinary built-in resolver.
5. The existing module cache owns the roots until runtime teardown. There are no
   per-call persistent handles, new worker tasks, or new cancellation responsibilities.

A deleted cwd produces a catchable `cannot read current directory` Error. An
unrepresentable cwd produces a `not valid UTF-8` Error. Explicit absolute anchors
can continue working after cwd is deleted. There is no process-global cwd mutation
API in JavaScript yet; an embedding host changing cwd changes later utility results.

The selected signatures follow the
[Node v24.11.0 path reference](https://nodejs.org/download/release/v24.11.0/docs/api/path.html).
Local compatibility evidence uses the installed Node version below; newer reserved
device rules and other release-specific behavior are not covered by a full parity
claim. Raw JS still runs on V8; no speedup is inferred from these utilities.

## Local evidence

Ten integration tests on macOS arm64 cover aliases, static/CommonJS imports,
captured bindings, POSIX/Windows roots, colon protection, raw parsing, suffixes,
format priority, cwd resolution, validation, captured intrinsics, and deleted-cwd
recovery. A Linux-only invalid-byte cwd fixture is present but was not run locally:
the macOS filesystem rejects that filename before the runtime can inspect it.
No Windows/Linux build support is established by these results.

A deterministic corpus of 1,760 entries matched local Node v20.19.4 for both
variants, including 400 generated paths and fixed edge cases. This is selected
correctness evidence, not package compatibility or performance data.

```sh
cargo build --locked
node tests/fixtures/path-conformance.cjs > /tmp/rustyjs-path-node.json
target/debug/rustyjs tests/fixtures/path-conformance.cjs > /tmp/rustyjs-path-runtime.json
cmp /tmp/rustyjs-path-node.json /tmp/rustyjs-path-runtime.json
cargo test --locked --test paths
```

Checks run locally; GitHub Actions remains idle.
