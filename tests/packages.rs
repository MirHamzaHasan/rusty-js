mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "rustyjs-packages-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, source).unwrap();
        path
    }

    fn package(&self, name: &str, manifest: &str) {
        self.write(&format!("node_modules/{name}/package.json"), manifest);
    }

    fn succeeds(&self, source: &str, expected: &[u8]) {
        let path = self.write("app/main.mjs", source);
        let output = common::run(&[path.to_str().unwrap()]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, expected);
        assert!(output.stderr.is_empty());
    }

    fn fails(&self, source: &str, message: &str) {
        let path = self.write("app/main.mjs", source);
        let output = common::run(&[path.to_str().unwrap()]);
        assert_eq!(output.status.code(), Some(1));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(message), "expected {message:?}: {error}");
        assert!(!error.contains("panicked"), "{error}");
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn packages_share_static_dynamic_and_direct_file_identity_with_host_await() {
    let project = Project::new();
    project.package(
        "answer",
        r#"{"type":"module","exports":{ ".":"./index.js", "./extra":"./extra.mjs" }}"#,
    );
    project.write("node_modules/answer/index.js", "import {sleep} from 'rustyjs:timers'; await sleep(1); globalThis.runs = (globalThis.runs ?? 0) + 1; export const answer = 42;");
    project.write(
        "node_modules/answer/extra.mjs",
        "export {answer} from './index.js';",
    );
    project.succeeds("import * as a from 'answer'; import * as direct from '../node_modules/answer/index.js'; import {answer} from 'answer/extra'; const [b,c] = await Promise.all([import('answer'), import('answer')]); if (a !== b || b !== c || a !== direct || runs !== 1 || answer !== 42) throw Error('package identity'); console.log('package', answer);", b"package 42\n");
}

#[test]
fn scoped_and_nested_dependencies_use_the_nearest_installed_version() {
    let project = Project::new();
    project.package("@scope/value", r#"{"exports":"./index.mjs"}"#);
    project.write(
        "node_modules/@scope/value/index.mjs",
        "export const value = 40;",
    );
    project.package("version", r#"{"main":"index.mjs"}"#);
    project.write("node_modules/version/index.mjs", "export const value = 1;");
    project.package("outer", r#"{"exports":"./lib/main.mjs"}"#);
    project.write(
        "node_modules/outer/lib/main.mjs",
        "export {value} from 'version';",
    );
    project.write(
        "node_modules/outer/node_modules/version/package.json",
        r#"{"main":"index.mjs"}"#,
    );
    project.write(
        "node_modules/outer/node_modules/version/index.mjs",
        "export const value = 2;",
    );
    project.succeeds("import {value as scoped} from '@scope/value'; import {value as nearest} from 'outer'; import {value as root} from 'version'; console.log(scoped, nearest, root);", b"40 2 1\n");
}

#[test]
fn conditional_exports_respect_order_nested_fallback_and_active_conditions() {
    let project = Project::new();
    project.package(
        "ordered",
        r#"{"exports":{"default":"./fallback.mjs","import":"./wrong.mjs"}}"#,
    );
    project.write(
        "node_modules/ordered/fallback.mjs",
        "export const value = 42;",
    );
    project.package("nested", r#"{"exports":{"node":"./wrong.mjs","require":"./wrong.cjs","import":{"browser":"./wrong.mjs"},"default":{"import":"./right.mjs"}}}"#);
    project.write("node_modules/nested/right.mjs", "export const value = 42;");
    project.package(
        "nonindex",
        r#"{"exports":{"01":"./wrong.mjs","4294967295":"./wrong.mjs","default":"./right.mjs"}}"#,
    );
    project.write(
        "node_modules/nonindex/right.mjs",
        "export const value = 42;",
    );
    project.succeeds(
        "import {value as a} from 'ordered'; import {value as b} from 'nested'; import {value as c} from 'nonindex'; console.log(a, b, c);",
        b"42 42 42\n",
    );
}

#[test]
fn exports_encapsulation_and_null_conditions_do_not_fall_back() {
    let project = Project::new();
    project.package("private", r#"{"main":"./ok.mjs","exports":{".":{"import":null,"default":"./ok.mjs"},"./blocked":null,"./public":"./ok.mjs"}}"#);
    project.write("node_modules/private/ok.mjs", "export const value = 42;");
    project.succeeds("for (const name of ['private', 'private/blocked', 'private/ok.mjs']) { try { await import(name); throw Error('unexported package path accepted'); } catch (error) { if (!(error instanceof TypeError) || !error.message.includes('not exported')) throw error; } } const {value} = await import('private/public'); console.log(value);", b"42\n");
    project.fails("import 'private/blocked';", "not exported");
}

#[test]
fn main_index_legacy_subpaths_and_self_reference_are_explicit() {
    let project = Project::new();
    project.package(
        "legacy",
        r#"{"type":"module","main":"entry.js","exports":null}"#,
    );
    project.write("node_modules/legacy/entry.js", "export const value = 42;");
    project.package("indexed", r#"{"type":"module"}"#);
    project.write("node_modules/indexed/index.js", "export const value = 42;");
    project.write("node_modules/plain/deep.mjs", "export const value = 42;");
    project.write(
        "package.json",
        r#"{"name":"self","exports":{"./entry":"./self.mjs"}}"#,
    );
    project.write("self.mjs", "export const value = 42;");
    project.succeeds("import {value as a} from 'legacy'; import {value as b} from 'indexed'; import {value as c} from 'plain/deep.mjs'; import {value as d} from 'self/entry'; console.log(a,b,c,d);", b"42 42 42 42\n");
}

#[test]
fn unsupported_commonjs_and_nested_scope_formats_fail_before_execution() {
    let project = Project::new();
    project.write("package.json", r#"{"type":"module"}"#);
    project.package("untyped", r#"{"main":"index.js"}"#);
    project.write(
        "node_modules/untyped/index.js",
        "globalThis.badFormatExecuted = true;",
    );
    project.package("typed", r#"{"type":"module","exports":"./main.js"}"#);
    project.write("node_modules/typed/main.js", "import './nested/value.js';");
    project.write(
        "node_modules/typed/nested/package.json",
        r#"{"type":"commonjs"}"#,
    );
    project.write(
        "node_modules/typed/nested/value.js",
        "globalThis.badFormatExecuted = true;",
    );
    project.package("cjs", r#"{"exports":"./index.cjs"}"#);
    project.write(
        "node_modules/cjs/index.cjs",
        "globalThis.badFormatExecuted = true;",
    );
    project.succeeds("for (const name of ['untyped', 'typed', 'cjs']) { try { await import(name); throw Error('accepted CommonJS'); } catch (error) { if (!(error instanceof TypeError) || !error.message.includes('CommonJS')) throw error; } } if (globalThis.badFormatExecuted) throw Error('executed unsupported format'); console.log('formats rejected');", b"formats rejected\n");
    project.fails("import '../node_modules/cjs/index.cjs';", "CommonJS");
}

#[test]
fn malformed_metadata_and_unsupported_exports_are_controlled() {
    let project = Project::new();
    for (manifest, message) in [
        ("{", "package.json"),
        ("[]", "JSON object"),
        (r#"{"type":"other"}"#, "package type"),
        (r#"{"exports":[42,"../outside.mjs"]}"#, "must start with ./"),
        (
            r#"{"exports":{"./**":"./*.mjs"}}"#,
            "invalid package map key",
        ),
        (
            r#"{"exports":{".":"./index.mjs","import":"./index.mjs"}}"#,
            "cannot mix",
        ),
        (
            r#"{"exports":{"0":"./index.mjs","default":"./index.mjs"}}"#,
            "condition key",
        ),
        (r#"{"main":42}"#, "main must be a string"),
    ] {
        project.package("bad", manifest);
        project.fails("import 'bad';", message);
    }
}

#[test]
fn invalid_targets_and_package_paths_cannot_escape_or_trigger_fallback() {
    let project = Project::new();
    project.write("outside.mjs", "throw Error('escaped package');");
    for target in [
        "../outside.mjs",
        "./../outside.mjs",
        "./%2e%2e/outside.mjs",
        "./NODE_MODULES/child.mjs",
        "./node%5fmodules/child.mjs",
        "./path%2fchild.mjs",
        "./path%5cchild.mjs",
        "./path//child.mjs",
        "./index.mjs?x",
        "./index.mjs#x",
    ] {
        project.package(
            "escape",
            &serde_json::json!({"exports": target}).to_string(),
        );
        project.fails("import 'escape';", "package");
    }
    project.succeeds("for (const name of ['pkg/../outside.mjs', '@scope', 'pkg//file.mjs', 'pkg/%2e%2e/file.mjs', '#internal']) { try { await import(name); throw Error('accepted invalid package path'); } catch (error) { if (!(error instanceof TypeError)) throw error; } } console.log('paths rejected');", b"paths rejected\n");
}

#[test]
fn nearest_package_failure_and_exact_files_do_not_guess_other_entries() {
    let project = Project::new();
    project.package("pkg", r#"{"main":"index.mjs"}"#);
    project.write(
        "node_modules/pkg/index.mjs",
        "throw Error('fell through to parent version');",
    );
    project.write(
        "app/node_modules/pkg/package.json",
        r#"{"main":"missing.mjs"}"#,
    );
    project.fails("import 'pkg';", "missing.mjs");
    project.package("guess", r#"{"main":"entry"}"#);
    project.write("node_modules/guess/entry.js", "export const value = 42;");
    project.fails("import 'guess';", "cannot load module");
    project.package("folder", r#"{"exports":"./dir"}"#);
    project.write(
        "node_modules/folder/dir/index.mjs",
        "export const value = 42;",
    );
    project.fails("import 'folder';", "not a regular file");
    project.package("json", r#"{"exports":"./data.json"}"#);
    project.write("node_modules/json/data.json", "{}");
    project.fails("import 'json';", "unsupported imported module format");
}

#[test]
fn escaped_package_targets_and_subpaths_work_without_a_cwd_base() {
    let project = Project::new();
    project.package("escaped", r#"{"exports":"./space%20%23%20caf%C3%A9.mjs"}"#);
    project.write(
        "node_modules/escaped/space # café.mjs",
        "export const value = 42;",
    );
    project.write("node_modules/plain/.hidden.mjs", "export const value = 42;");
    project.write(
        "node_modules/plain/space # café.mjs",
        "export const value = 42;",
    );
    project.succeeds("import {value as a} from 'escaped'; import {value as b} from 'plain/space%20%23%20caf%C3%A9.mjs'; import {value as c} from 'plain/.hidden.mjs'; console.log(a,b,c);", b"42 42 42\n");
    let output = common::run(&[
        "-e",
        "import('escaped').catch(error => { if (!(error instanceof TypeError) || !error.message.includes('file referrer')) throw error; console.log('no inline package base'); });",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"no inline package base\n");
}

#[cfg(unix)]
#[test]
fn linked_packages_share_canonical_identity_and_export_targets_stay_inside() {
    let project = Project::new();
    project.write("real/package.json", r#"{"exports":"./index.mjs"}"#);
    project.write("real/index.mjs", "export const value = 42;");
    std::fs::create_dir_all(project.0.join("node_modules")).unwrap();
    std::os::unix::fs::symlink(
        project.0.join("real"),
        project.0.join("node_modules/linked"),
    )
    .unwrap();
    project.succeeds("import * as a from 'linked'; import * as b from '../real/index.mjs'; if (a !== b) throw Error('symlink identity'); console.log(a.value);", b"42\n");
    project.write("outside.mjs", "throw Error('escaped');");
    std::os::unix::fs::symlink(
        project.0.join("outside.mjs"),
        project.0.join("real/escape.mjs"),
    )
    .unwrap();
    project.write("real/package.json", r#"{"exports":"./escape.mjs"}"#);
    project.fails(
        "import 'linked';",
        "escapes its canonical package directory",
    );
}

#[test]
fn export_patterns_use_exact_prefix_and_trailer_specificity() {
    let project = Project::new();
    project.package(
        "patterns",
        r#"{"exports":{
        "./features/*":"./src/*.mjs",
        "./features/*.js":"./special/*.mjs",
        "./features/deep/*":"./deep/*.mjs",
        "./features/private/*":null,
        "./features/private/open":"./src/open.mjs",
        "./repeat/*":"./copies/*/*.mjs"
    }}"#,
    );
    project.write(
        "node_modules/patterns/src/basic.mjs",
        "export const value = 1;",
    );
    project.write(
        "node_modules/patterns/special/basic.mjs",
        "export const value = 2;",
    );
    project.write(
        "node_modules/patterns/deep/item.js.mjs",
        "export const value = 3;",
    );
    project.write(
        "node_modules/patterns/src/open.mjs",
        "export const value = 4;",
    );
    project.write(
        "node_modules/patterns/copies/name/name.mjs",
        "export const value = 5;",
    );
    project.succeeds("import * as basic from 'patterns/features/basic'; import {value as suffix} from 'patterns/features/basic.js'; import {value as deep} from 'patterns/features/deep/item.js'; import {value as exact} from 'patterns/features/private/open'; import {value as repeated} from 'patterns/repeat/name'; if (basic !== await import('patterns/features/basic')) throw Error('pattern cache identity'); console.log(basic.value,suffix,deep,exact,repeated);", b"1 2 3 4 5\n");
    project.fails("import 'patterns/features/private/secret';", "not exported");
    project.fails(
        "import 'patterns/features/deep/missing';",
        "cannot load module",
    );
}

#[test]
fn arrays_skip_invalid_targets_nulls_and_unmatched_conditions() {
    let project = Project::new();
    project.package(
        "arrays",
        r#"{"exports":{
        ".":[42,"../outside.mjs",null,{"browser":"./wrong.mjs"},["./ok.mjs"]],
        "./pattern/*":["../invalid/*.mjs",{"import":[null,"./*.mjs"]}],
        "./blocked":{"import":[],"default":"./ok.mjs"},
        "./null":{"import":null,"default":"./ok.mjs"},
        "./empty":[],
        "./fallback":{"import":[{"browser":"./wrong.mjs"}],"default":"./ok.mjs"}
    }}"#,
    );
    project.write("node_modules/arrays/ok.mjs", "export const value = 42;");
    project.succeeds("import * as root from 'arrays'; import * as pattern from 'arrays/pattern/ok'; import * as fallback from 'arrays/fallback'; if (root !== pattern || root !== fallback) throw Error('array identity'); for (const name of ['arrays/blocked','arrays/null','arrays/empty']) { try { await import(name); throw Error('array block bypassed'); } catch(error) { if (!(error instanceof TypeError) || !error.message.includes('not exported')) throw error; } } console.log(root.value);", b"42\n");
}

#[test]
fn array_selection_does_not_retry_missing_files_formats_or_bad_config() {
    let project = Project::new();
    project.write(
        "node_modules/choice/good.mjs",
        "throw Error('unexpected filesystem fallback');",
    );
    project.write(
        "node_modules/choice/bad.cjs",
        "throw Error('executed CommonJS');",
    );
    project.write("node_modules/choice/syntax.mjs", "export const = ;");
    for (targets, message) in [
        (r#"["./missing.mjs","./good.mjs"]"#, "missing.mjs"),
        (r#"["./bad.cjs","./good.mjs"]"#, "CommonJS"),
        (r#"["./syntax.mjs","./good.mjs"]"#, "SyntaxError"),
        (r#"[{"0":"./good.mjs"},"./good.mjs"]"#, "condition key"),
        (r#"[null,42]"#, "invalid package target"),
        (r#"[42,null]"#, "not exported"),
    ] {
        project.package("choice", &format!(r#"{{"exports":{targets}}}"#));
        project.fails("import 'choice';", message);
    }
}

#[test]
fn pattern_templates_and_captures_validate_before_url_normalization() {
    let project = Project::new();
    project.package("safe", r#"{"exports":{"./*":"./src/*/../ok.mjs"}}"#);
    project.fails("import 'safe/value';", "invalid package target segment");
    project.package("safe", r#"{"exports":{"./*":"./src/*.mjs"}}"#);
    project.succeeds("for (const name of ['safe/%2e%2e/outside', 'safe/path%2fchild', 'safe/node%5fmodules/file', 'safe/value/']) { try { await import(name); throw Error('invalid capture accepted'); } catch(error) { if (!(error instanceof TypeError)) throw error; } } console.log('captures validated');", b"captures validated\n");
    project.package("safe", r#"{"exports":{"./%2e%2e/*":"./*.mjs"}}"#);
    project.fails("import 'safe/value';", "invalid package map key");
}

#[test]
fn patterns_handle_encoded_unicode_and_target_depth_is_bounded() {
    let project = Project::new();
    project.package("unicode", r#"{"exports":{"./café/*":"./src/*.mjs"}}"#);
    project.write(
        "node_modules/unicode/src/space # café.mjs",
        "export const value = 42;",
    );
    project.succeeds(
        "import {value} from 'unicode/café/space%20%23%20caf%C3%A9'; console.log(value);",
        b"42\n",
    );
    let mut target = serde_json::json!("./index.mjs");
    for _ in 0..34 {
        target = serde_json::json!([target]);
    }
    project.package("depth", &serde_json::json!({"exports": target}).to_string());
    project.fails("import 'depth';", "nesting exceeds 32");
}

#[test]
fn imports_maps_share_file_and_builtin_identity_and_resolve_from_package_root() {
    let project = Project::new();
    project.write(
        "package.json",
        r##"{"type":"module","imports":{
        "#answer":"./lib/answer.js",
        "#feature/*":"./lib/*.js",
        "#timers":"rustyjs:timers",
        "#choice":{"require":"./wrong.cjs","import":[null,"../invalid.mjs","./lib/answer.js"]}
    }}"##,
    );
    project.write(
        "lib/answer.js",
        "import {sleep} from '#timers'; await sleep(0); export const value = 42;",
    );
    project.succeeds("import * as a from '#answer'; import * as b from '../lib/answer.js'; import * as c from '#feature/answer'; import * as d from '#choice'; import * as native from 'rustyjs:timers'; import * as aliased from '#timers'; if (a !== b || a !== c || a !== d || a !== await import('#answer') || native !== aliased || native !== await import('#timers')) throw Error('imports map identity'); console.log(a.value);", b"42\n");
    let classic = project.write(
        "app/classic.js",
        "import('#answer').then(({value}) => console.log('classic imports', value));",
    );
    let output = common::run(&[classic.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"classic imports 42\n");
}

#[test]
fn imports_external_aliases_use_manifest_dependencies_and_exports() {
    let project = Project::new();
    project.write(
        "package.json",
        r##"{"imports":{"#dependency":"value","#subpath/*":"value/*"}}"##,
    );
    project.package(
        "value",
        r#"{"exports":{".":"./index.mjs","./extra":"./extra.mjs"}}"#,
    );
    project.write("node_modules/value/index.mjs", "export const value = 42;");
    project.write("node_modules/value/extra.mjs", "export const value = 84;");
    project.write(
        "app/node_modules/value/package.json",
        r#"{"exports":"./index.mjs"}"#,
    );
    project.write(
        "app/node_modules/value/index.mjs",
        "export const value = 99;",
    );
    project.succeeds("import {value as aliased} from '#dependency'; import {value as ordinary} from 'value'; import {value as subpath} from '#subpath/extra'; console.log(aliased, ordinary, subpath);", b"42 99 84\n");
    project.fails("import '#subpath/private';", "not exported");
}

#[test]
fn imports_scopes_stop_at_nearest_manifest_and_dependency_boundaries() {
    let project = Project::new();
    project.write("package.json", r##"{"imports":{"#value":"./root.mjs"}}"##);
    project.write("root.mjs", "export const value = 1;");
    project.write(
        "nested/package.json",
        r##"{"imports":{"#value":"./inner.mjs"}}"##,
    );
    project.write("nested/inner.mjs", "export const value = 2;");
    project.write("nested/main.mjs", "export {value} from '#value';");
    project.succeeds("import {value as a} from '#value'; import {value as b} from '../nested/main.mjs'; console.log(a,b);", b"1 2\n");
    project.write("nested/package.json", "{}");
    project.fails(
        "import '../nested/main.mjs';",
        "not defined in the nearest scope",
    );
    project.write("node_modules/dependency/main.mjs", "import '#value';");
    project.fails("import 'dependency/main.mjs';", "no package scope");
}

#[test]
fn invalid_imports_maps_and_requests_are_catchable_without_alias_recursion() {
    let project = Project::new();
    project.write("package.json", r##"{"imports":{"#loop":"#loop","#blocked":null,"#bad":"https://example.invalid/code.mjs","#fallback":["#loop","./ok.mjs"]}}"##);
    project.write("ok.mjs", "export const value = 42;");
    project.succeeds("for (const name of ['#','#/bad','#path/../outside','#path%2fchild','#loop','#blocked','#missing','#bad']) { try { await import(name); throw Error('invalid imports accepted'); } catch(error) { if (!(error instanceof TypeError)) throw error; } } const {value} = await import('#fallback'); console.log(value);", b"42\n");
    for (manifest, message) in [
        (r#"{"imports":[]}"#, "object map"),
        (
            r##"{"imports":{"value":"./ok.mjs","#ok":"./ok.mjs"}}"##,
            "invalid imports key",
        ),
        (
            r##"{"imports":{"#/bad":"./ok.mjs"}}"##,
            "invalid imports key",
        ),
    ] {
        project.write("package.json", manifest);
        project.fails("import '#ok';", message);
    }
    let output = common::run(&[
        "-e",
        "import('#ok').catch(error => { if (!(error instanceof TypeError) || !error.message.includes('file referrer')) throw error; console.log('no inline imports base'); });",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"no inline imports base\n");
}

#[test]
fn imports_do_not_retry_unavailable_selected_dependencies_or_builtins() {
    let project = Project::new();
    project.write(
        "ok.mjs",
        "throw Error('incorrectly retried selected target');",
    );
    for (target, message) in [
        (
            r#"["not-installed","./ok.mjs"]"#,
            "was not found in node_modules",
        ),
        (
            r#"["rustyjs:missing","./ok.mjs"]"#,
            "unknown built-in module",
        ),
        (r#"["./missing.mjs","./ok.mjs"]"#, "cannot load module"),
        (
            r#""./%2e%2e/outside.mjs""#,
            "invalid package target segment",
        ),
    ] {
        project.write(
            "package.json",
            &format!(r##"{{"imports":{{"#value":{target}}}}}"##),
        );
        project.fails("import '#value';", message);
    }
}
