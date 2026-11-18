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
    project.succeeds(
        "import {value as a} from 'ordered'; import {value as b} from 'nested'; console.log(a, b);",
        b"42 42\n",
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
        (r#"{"exports":["./index.mjs"]}"#, "fallback arrays"),
        (r#"{"exports":{"./*":"./*.mjs"}}"#, "patterns"),
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
