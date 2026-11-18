# Package-aware ESM resolution

## Resolution contract

This slice adds installed ESM package loading, not package installation or full
Node compatibility. Static and dynamic imports use the same resolver and module
cache. Packages must already exist on disk; RustyJS never downloads dependencies.

- Unscoped (`pkg`) and scoped (`@scope/pkg`) names search `node_modules` from the
  defining file's canonical directory upward. The first package directory wins;
  a broken nearest package does not fall through to another version higher up.
- Exact and wildcard package subpaths are supported. With `exports`, only declared paths are
  accessible by package name. Without it, explicit subpaths address exact files.
- `exports` supports a string or array root entry, exact `.` / `./subpath` maps,
  single-wildcard keys, null blocking, and nested conditional objects. Active conditions are `import` and
  `default`, considered in manifest insertion order. Unknown conditions are skipped.
  `node`, `require`, and `browser` are not active RustyJS conditions.
- Exact keys take precedence over patterns. Matching patterns prefer the longest
  prefix before `*`, then the longest suffix; captures must be nonempty and can
  contain multiple path segments. Every `*` in the selected target is replaced.
- Fallback arrays try targets in order, skipping invalid target shapes, null
  entries, and unmatched conditions. Empty arrays block. Once a valid target is
  selected, missing files, unsupported formats, compilation errors, unavailable
  dependencies, and symlink escapes fail without trying another array item.
  Invalid map/condition configuration and depth errors also stop immediately.
- File targets must start `./` and cannot contain traversal, empty segments, or
  `node_modules` segments, including encoded variants. Targets cannot escape the
  canonical package directory through symlinks. Queries/fragments are unsupported.
  The same target checks apply to main entries and legacy subpaths.
- Without non-null `exports`, a package root uses an explicit string `main`, or
  `index.js` if `main` is absent. No extension guessing, directory indexes beyond
  that package-root default, or fallback from a broken `main` is performed.
- A nearest package scope with matching `name` and non-null `exports` supports
  self-reference. Its `imports` object defines package-local `#names` with exact
  keys, patterns, arrays, and conditions. Local targets are relative to the package
  root; external package targets resolve from the manifest's dependency location.
  `rustyjs:*` targets share built-in module identity. A nested manifest or
  `node_modules` boundary prevents accidental inheritance of another scope's map.
- `#imports` targets that reference another `#name` are unsupported, so alias maps
  cannot recurse. Ordinary module cycles still use V8's graph semantics. Other
  URL schemes, package-manager aliases, and custom conditions remain unsupported.
  Inline `<eval>` has no file base for package lookup or `#imports`.
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
Array selection also follows the pinned
[Node 24 resolver implementation](https://github.com/nodejs/node/blob/v24.11.0/lib/internal/modules/esm/resolve.js),
which distinguishes invalid targets from missing files and configuration errors.

## Example

```sh
cargo run --locked -- examples/packages/main.mjs
cargo run --locked -- examples/packages/maps.mjs
```

The example uses its package's own `name` and `exports` to import `.js` files under
`type: module`, including a subpath and a dynamic import. It prints
`package imports: 42 84`. Installed dependencies use the same entry rules after
`node_modules` lookup; the tests create these layouts in temporary directories.
The maps example prints `package maps: 42`, combining a wildcard public export,
array fallback from an invalid target, and a private `#answer` import. Both routes
return the same cached namespace. `#timers` aliases the native timer module.

## Pattern and fallback details

For `"./features/*.js": "./src/*.mjs"`, importing `pkg/features/deep/item.js`
selects `./src/deep/item.mjs`. A more specific `./features/private/*` null mapping
blocks that subtree, while an exact key can explicitly expose one file inside it.
The loader validates every map key, plus target templates and expanded targets,
before URL normalization. Multiple wildcards in a key and invalid/traversing key
segments are configuration errors even if that key is not selected.

`["../invalid.mjs", "./ok.mjs"]` selects the second item. In contrast,
`["./missing.mjs", "./ok.mjs"]` selects the first path and then fails if the file
does not exist. An array is target selection, not filesystem probing.

A null selected directly by an active condition blocks later condition branches.
Inside an array, null entries allow later array items; if no valid target wins,
the last null or invalid-target error determines the result. Unmatched conditions
leave that result unchanged. This distinction keeps nested conditions and arrays
from accidentally bypassing private or unsupported paths.

`imports` keys must begin with `#` and cannot be `#` or begin with `#/`. A missing
entry does not fall through to a parent package's map. External aliases still obey
the target dependency's exports and format rules. Direct `#`-to-`#` alias chains
and URL targets are rejected as invalid target shapes; an array may supply a
supported alternative.

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
   null outside an array blocks fallback; an unmatched nested condition lets its
   parent continue. Target selection distinguishes invalid-target errors (which
   arrays may skip) from configuration errors (which they must preserve).
4. The selected relative target is decoded and checked for invalid segments before
   URL joining can normalize traversal away. Canonicalization then checks the
   regular file and confines package entry targets to the canonical package root.
   Bare package names cannot contain percent escapes; subpaths and targets may use
   percent encoding for names with spaces, Unicode, or `#`.
5. Format validation uses the target file's nearest package scope, including nested
   manifests, rather than assuming that an exported file is ESM. `.mjs` remains
   ESM even in a commonjs package. Explicit file imports use the same format check.
6. `#imports` starts at the nearest manifest and uses the same matching/selection
   helpers. A relative target returns a file ID, a package alias reuses installed
   package resolution from the manifest, and a `rustyjs:*` alias returns a built-in
   ID. No redirect re-enters V8 or recursively follows another `#alias`.
7. A successful resolution returns a canonical file or built-in ID to the existing graph
   loader. V8 still compiles, links, and evaluates; package and direct file aliases
   share cached module identity. No V8 state lives in the package resolver.

Metadata reads and directory traversal are synchronous on the isolate thread, as
are current source-file reads. Manifests are not cached yet. A new dynamic lookup
can observe changed package routing, while an already-cached graph retains its
resolved edges and compiled modules. Use a fresh runtime for a fully fresh graph;
this is not hot reloading or a serialized code cache.

The canonical target check is a loader policy, not a filesystem sandbox. Guest
code can still explicitly import other permitted local files; execution/resource
limits remain future work. Combined array/condition depth is limited to 32 recursive
steps, but package size, source size, and execution are not otherwise budgeted.

## Local verification

The suite creates real temporary `node_modules` trees and invokes the CLI from a
different cwd. Coverage includes scoped/nested versions, root/main/index/subpath
entries, self-reference, conditional ordering and null blocks, shared namespace
identity, host-backed top-level await, encoded paths, malformed metadata,
unsupported export forms, blocked formats, traversal, failed-nearest-package
behavior, and Unix symlink identity/confinement. A library test repairs invalid
metadata and successfully retries the graph in the same runtime.
Additional tests cover exact/prefix/suffix specificity, repeated target
substitution, null exclusions, invalid array targets, condition-index keys,
missing-file/format failures without array retry, encoded captures, nesting limits,
private imports, native aliases, manifest-relative external dependencies, nested
scope boundaries, classic/inline behavior, and rejected alias cycles.

These are RustyJS resolver tests, not evidence of broad Node package compatibility.
CommonJS execution and real package test-suite compatibility remain separate
milestones. All checks for this slice run locally; GitHub CI stays idle.
