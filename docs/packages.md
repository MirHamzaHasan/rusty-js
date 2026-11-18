# Package-aware ESM resolution

## Resolution contract

This slice adds installed ESM package loading, not package installation or full
Node compatibility. Static and dynamic imports use the same resolver and module
cache. Packages must already exist on disk; RustyJS never downloads dependencies.

- Unscoped (`pkg`) and scoped (`@scope/pkg`) names search `node_modules` from the
  defining file's canonical directory upward. The first package directory wins;
  a broken nearest package does not fall through to another version higher up.
- Exact package subpaths are supported. With `exports`, only declared paths are
  accessible by package name. Without it, explicit subpaths address exact files.
- `exports` supports a string root entry, exact `.` / `./subpath` maps, null
  blocking, and nested conditional objects. Active conditions are `import` and
  `default`, considered in manifest insertion order. Unknown conditions are skipped.
  `node`, `require`, and `browser` are not active RustyJS conditions.
- Export patterns and fallback arrays are unsupported and produce explicit errors.
  Targets must start `./` and cannot contain traversal, empty segments, or
  `node_modules` segments, including encoded variants. Targets cannot escape the
  canonical package directory through symlinks. Queries/fragments are unsupported.
  The same target checks apply to main entries and legacy subpaths.
- Without non-null `exports`, a package root uses an explicit string `main`, or
  `index.js` if `main` is absent. No extension guessing, directory indexes beyond
  that package-root default, or fallback from a broken `main` is performed.
- A nearest package scope with matching `name` and non-null `exports` supports
  self-reference. `#imports`, package manager aliases, and custom conditions are
  unsupported. Inline `<eval>` has no file base for package lookup.
- Imported files must be `.mjs`, or `.js` with nearest package.json
  `"type": "module"`. The scope search stops at `node_modules`; a parent app's
  type cannot turn an untyped dependency into ESM. Nested manifests start a new
  scope. There is no syntax detection or CommonJS, JSON, native-addon, or TS loader.
- CLI `.js` entry behavior stays classic unless `--module` is supplied. An explicit
  module entry can force ESM source parsing for another extension, except `.cjs`;
  its imports still follow the format rules above.

Package metadata must be valid UTF-8 JSON objects. Missing manifests are distinct
from malformed or unreadable manifests; the latter are reported with their path.
`type`, when present, must be `module` or `commonjs`. Format/read/resolution errors
are ordinary module diagnostics or catchable TypeError rejections for `import()`.
V8 compilation, linking, evaluation, caching, and cancellation keep their existing
[module semantics](modules.md).

The subset follows the structure of the
[Node 24 ESM resolution specification](https://nodejs.org/download/release/v24.11.0/docs/api/esm.html#resolution-algorithm-specification)
and [package entry rules](https://nodejs.org/download/release/v24.11.0/docs/api/packages.html#package-entry-points).
The explicit omissions and stricter format/target checks above are RustyJS policy.

## Example

```sh
cargo run --locked -- examples/packages/main.mjs
```

The example uses its package's own `name` and `exports` to import `.js` files under
`type: module`, including a subpath and a dynamic import. It prints
`package imports: 42 84`. Installed dependencies use the same entry rules after
`node_modules` lookup; the tests create these layouts in temporary directories.

## Implementation walkthrough

1. The module resolver recognizes built-ins and explicit file references first.
   Bare names reach `packages::resolve`, which validates package/subpath syntax
   and requires an existing file referrer. No lookup depends on the process cwd.
2. Package self-reference is checked against the nearest scope. Otherwise the
   resolver walks ancestor directories, skipping redundant `node_modules` levels,
   and stops at the nearest installed package. Read errors do not silently become
   missing-package fallbacks.
3. `read_manifest` parses JSON with object order preserved. That matters for
   conditional exports: a `default` appearing before `import` wins. A selected
   null blocks fallback; an unmatched nested condition lets its parent continue.
4. The selected relative target is decoded and checked for invalid segments before
   URL joining can normalize traversal away. Canonicalization then checks the
   regular file and confines package entry targets to the canonical package root.
   Bare package names cannot contain percent escapes; subpaths and targets may use
   percent encoding for names with spaces, Unicode, or `#`.
5. Format validation uses the target file's nearest package scope, including nested
   manifests, rather than assuming that an exported file is ESM. `.mjs` remains
   ESM even in a commonjs package. Explicit file imports use the same format check.
6. A successful resolution returns a canonical file ID to the existing graph
   loader. V8 still compiles, links, and evaluates; package and direct file aliases
   share cached module identity. No V8 state lives in the package resolver.

Metadata reads and directory traversal are synchronous on the isolate thread, as
are current source-file reads. Manifests are not cached yet. A new dynamic lookup
can observe changed package routing, while an already-cached graph retains its
resolved edges and compiled modules. Use a fresh runtime for a fully fresh graph;
this is not hot reloading or a serialized code cache.

The canonical target check is a loader policy, not a filesystem sandbox. Guest
code can still explicitly import other permitted local files; execution/resource
limits remain future work. Package condition depth is limited to 32 recursive
steps, but package size, source size, and execution are not otherwise budgeted.

## Local verification

The suite creates real temporary `node_modules` trees and invokes the CLI from a
different cwd. Coverage includes scoped/nested versions, root/main/index/subpath
entries, self-reference, conditional ordering and null blocks, shared namespace
identity, host-backed top-level await, encoded paths, malformed metadata,
unsupported export forms, blocked formats, traversal, failed-nearest-package
behavior, and Unix symlink identity/confinement. A library test repairs invalid
metadata and successfully retries the graph in the same runtime.

These are RustyJS resolver tests, not evidence of broad Node package compatibility.
CommonJS execution and real package test-suite compatibility remain separate
milestones. All checks for this slice run locally; GitHub CI stays idle.
