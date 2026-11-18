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
            "rustyjs-cjs-{}-{nonce}-{}",
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
    fn succeeds(&self, name: &str, source: &str, expected: &[u8]) {
        let path = self.write(name, source);
        let output = common::run(&[path.to_str().unwrap()]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, expected);
        assert!(output.stderr.is_empty());
    }
    fn fails(&self, name: &str, source: &str, message: &str) {
        let path = self.write(name, source);
        let output = common::run(&[path.to_str().unwrap()]);
        assert_eq!(output.status.code(), Some(1));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(message), "{error}");
        assert!(!error.contains("panicked"), "{error}");
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn wrapper_scope_exports_replacement_metadata_and_cache_identity() {
    let project = Project::new();
    project.write("child.js", "var privateValue = 7; globalThis.childRuns = (globalThis.childRuns ?? 0) + 1; exports.wrong = true; exports = {ignored: true}; module.exports = {value: 42, metadata: () => module.loaded && module.id === __filename && module.filename === __filename && require.main.filename.endsWith('main.cjs')};");
    project.succeeds("main.cjs", "if (this !== exports || arguments.length !== 5 || module.loaded !== false || require.main !== module || module.id !== '.' || module.require !== require || module.filename !== __filename || !__dirname || !__filename.endsWith('main.cjs')) throw Error('wrapper metadata'); const a = require('./child'); const b = require('./child.js'); if (a !== b || a.value !== 42 || a.wrong || a.ignored || childRuns !== 1 || !a.metadata() || globalThis.privateValue) throw Error('exports/cache/scope'); if (!require.resolve('./child').endsWith('child.js')) throw Error('resolve'); console.log(a.value);", b"42\n");
}

#[test]
fn circular_requires_observe_partial_exports_without_reexecution() {
    let project = Project::new();
    project.write("a.cjs", "exports.stage = 'partial'; const b = require('./b.cjs'); exports.observed = b.observed; exports.stage = 'complete';");
    project.write("b.cjs", "exports.observed = require('./a.cjs').stage;");
    project.succeeds("main.cjs", "const a = require('./a.cjs'); if (a.observed !== 'partial' || a.stage !== 'complete' || require('./a.cjs') !== a) throw Error('cycle'); console.log('cycle complete');", b"cycle complete\n");
}

#[test]
fn json_file_probes_directory_main_and_index_resolution_work() {
    let project = Project::new();
    project.write(
        "data.json",
        "\u{feff}{\"value\":42,\"__proto__\":{\"polluted\":true}}",
    );
    project.write("folder/package.json", r#"{"main":"lib"}"#);
    project.write("folder/lib/index.js", "module.exports = 84;");
    project.write("fallback/package.json", r#"{"main":"missing.js"}"#);
    project.write("fallback/index.json", "126");
    project.write(
        "node_modules/legacy/package.json",
        r#"{"main":"entry.cjs"}"#,
    );
    project.write("node_modules/legacy/entry.cjs", "module.exports = 168;");
    project.write("node_modules/single.js", "module.exports = 210;");
    project.succeeds("main.cjs", "const data = require('./data'); if (data !== require('./data.json') || data.value !== 42 || ({}).polluted) throw Error('JSON'); console.log(data.value, require('./folder'), require('./fallback'), require('legacy'), require('single'));", b"42 84 126 168 210\n");
    project.write("invalid.json", "{");
    project.succeeds("bad-json.cjs", "try { require('./invalid.json'); throw Error('accepted JSON'); } catch(error) { if (!(error instanceof SyntaxError) || !error.message.includes('invalid.json')) throw error; } console.log('caught JSON');", b"caught JSON\n");
}

#[test]
fn package_conditions_private_imports_and_native_timer_objects_are_shared() {
    let project = Project::new();
    project.write("package.json", r##"{"imports":{"#value":{"require":"./value.cjs","import":"./value.mjs"},"#timers":"rustyjs:timers"}}"##);
    project.write("value.cjs", "module.exports = 42;");
    project.write("value.mjs", "export const value = 84;");
    project.write(
        "node_modules/dual/package.json",
        r#"{"exports":{"require":"./entry.cjs","import":"./entry.mjs"}}"#,
    );
    project.write("node_modules/dual/entry.cjs", "module.exports = 21;");
    project.write("node_modules/dual/entry.mjs", "export const value = 168;");
    project.succeeds("main.cjs", "const native = require('#timers'); if (native !== require('rustyjs:timers') || native.sleep !== Rusty.sleep || require('#value') !== 42 || require('dual') !== 21) throw Error('require conditions'); import('#value').then(async ({value}) => { const dual = await import('dual'); const timers = await import('rustyjs:timers'); if (value !== 84 || dual.value !== 168 || timers.sleep !== native.sleep) throw Error('import conditions'); await native.sleep(0); console.log('conditions and timers'); });", b"conditions and timers\n");
}

#[test]
fn esm_default_bridge_and_require_share_cache_with_snapshot_reassignment() {
    let project = Project::new();
    project.write("value.cjs", "globalThis.cjsRuns = (globalThis.cjsRuns ?? 0) + 1; module.exports = {value:42, replace(){ module.exports = {value:84}; }};");
    project.succeeds("main.cjs", "const initial = require('./value.cjs'); import('./value.cjs').then(namespace => { if (namespace.default !== initial || Object.keys(namespace).join(',') !== 'default' || cjsRuns !== 1) throw Error('bridge identity'); initial.value = 43; if (namespace.default.value !== 43) throw Error('shared object'); initial.replace(); if (namespace.default !== initial || require('./value.cjs').value !== 84) throw Error('bridge snapshot'); console.log('shared bridge'); });", b"shared bridge\n");
    project.succeeds("esm.mjs", "import value from './value.cjs'; import * as namespace from './value.cjs'; if (value !== namespace.default || value.value !== 42) throw Error('static bridge'); console.log(value.value);", b"42\n");
    project.fails(
        "named.mjs",
        "import {value} from './value.cjs'; console.log(value);",
        "does not provide an export named 'value'",
    );
}

#[test]
fn failed_modules_retry_original_exceptions_and_keep_successful_dependencies() {
    let project = Project::new();
    project.write(
        "dependency.js",
        "globalThis.depRuns = (globalThis.depRuns ?? 0) + 1; module.exports = 42;",
    );
    project.write("bad.cjs", "require('./dependency'); globalThis.badRuns = (globalThis.badRuns ?? 0) + 1; throw globalThis.sentinel;");
    project.succeeds("main.cjs", "globalThis.sentinel = {message:'sentinel'}; for(let i=0;i<2;i++) { try { require('./bad.cjs'); throw Error('bad module succeeded'); } catch(error) { if(error !== sentinel) throw Error('lost thrown value'); } } if (badRuns !== 2 || depRuns !== 1) throw Error('failure cache'); console.log('retry works');", b"retry works\n");
    project.write("syntax.cjs", "const valid = 1;\nconst = ;\n");
    project.fails(
        "syntax-main.cjs",
        "require('./syntax.cjs');",
        "syntax.cjs:2:",
    );
    project.succeeds("rejected.mjs", "try { await import('./bad.cjs'); throw Error('bad import succeeded'); } catch(error) { if (error !== undefined) throw Error('lost undefined rejection'); } console.log('caught CJS import');", b"caught CJS import\n");
}

#[test]
fn cached_exports_getter_reentry_is_bounded_and_registry_borrows_are_released() {
    let project = Project::new();
    project.write(
        "recursive.cjs",
        "Object.defineProperty(module,'exports',{get(){ return require('./recursive.cjs'); }});",
    );
    project.write("valid.js", "module.exports = 42;");
    project.succeeds("main.cjs", "try { require('./recursive.cjs'); throw Error('unbounded getter accepted'); } catch(error) { if (!(error instanceof RangeError) && !error.message.includes('depth exceeds 64')) throw error; } if(require('./valid') !== 42) throw Error('loader not reusable'); console.log('reentry bounded');", b"reentry bounded\n");
}

#[test]
fn require_arguments_never_coerce_and_esm_boundary_is_explicit() {
    let project = Project::new();
    project.write(
        "esm.mjs",
        "globalThis.esmExecuted = true; export const value = 42;",
    );
    project.write("typed/package.json", r#"{"type":"module"}"#);
    project.write(
        "typed/value.js",
        "globalThis.esmExecuted = true; export const value = 42;",
    );
    project.succeeds("main.cjs", "for (const value of [undefined,42,{toString(){throw Error('coerced')}}]) { try { require(value); throw Error('bad argument accepted'); } catch(error) { if (!(error instanceof TypeError)) throw error; } } for(const path of ['./esm.mjs','./typed/value.js']) { try { require(path); throw Error('required ESM'); } catch(error) { if (!error.message.includes('use import()')) throw error; } } if(globalThis.esmExecuted) throw Error('executed ESM synchronously'); console.log('require boundary');", b"require boundary\n");
}

#[test]
fn commonjs_mode_is_explicit_for_js_and_bound_require_survives_later_callbacks() {
    let project = Project::new();
    project.write("nested/value.js", "module.exports = 42;");
    project.write(
        "nested/callback.cjs",
        "module.exports = () => require('./value');",
    );
    let path = project.write("main.js", "const callback = require('./nested/callback.cjs'); setTimeout(() => console.log(callback()), 0);");
    let output = common::run(&["--commonjs", path.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"42\n");
    assert_eq!(
        common::run(&[path.to_str().unwrap()]).status.code(),
        Some(1)
    );
    assert_eq!(common::run(&["--commonjs"]).status.code(), Some(1));
    project.succeeds(
        "shebang.cjs",
        "\u{feff}#!/usr/bin/env rustyjs\nmodule.exports = 42; console.log(module.exports);",
        b"42\n",
    );
}

#[test]
fn host_metadata_ignores_inherited_guest_setters_and_readonly_failure_is_catchable() {
    let project = Project::new();
    project.write("child.js", "module.exports = 42;");
    project.write(
        "readonly.cjs",
        "Object.defineProperty(module,'loaded',{value:false,configurable:false,writable:false});",
    );
    project.succeeds("main.cjs", "for(const key of ['exports','loaded','filename','id','require','main','resolve']) Object.defineProperty(Object.prototype,key,{configurable:true,get(){throw Error('inherited getter');},set(){throw Error('inherited setter');}}); if (require('./child') !== 42) throw Error('metadata poisoning'); try {require('./readonly.cjs'); throw Error('readonly loaded accepted');} catch(error) {if (!error.message.includes('module metadata')) throw error;} console.log('metadata safe');", b"metadata safe\n");
}

#[test]
fn create_require_from_esm_shares_modules_builtins_and_json_without_a_ghost_main() {
    let project = Project::new();
    project.write("value.cjs", "globalThis.factoryRuns = (globalThis.factoryRuns ?? 0) + 1; module.exports = {value:42, main: require.main};");
    project.write("data.json", "{\"value\":84}");
    project.succeeds("main.mjs", "import {createRequire} from 'rustyjs:module'; import value from './value.cjs'; const a = createRequire(import.meta.url); const b = createRequire(import.meta.url); if (a === b || a('./value.cjs') !== value || value.main !== undefined || a.main !== undefined || factoryRuns !== 1 || a('rustyjs:module').createRequire !== createRequire || a('./data').value !== 84) throw Error('factory cache/main'); const timers = await import('rustyjs:timers'); if (a('rustyjs:timers').sleep !== timers.sleep) throw Error('factory native identity'); await timers.sleep(0); if (a('./value.cjs') !== b('./value.cjs') || !a.resolve('./value.cjs').endsWith('value.cjs')) throw Error('factory lifetime'); console.log('factory', value.value);", b"factory 42\n");
}

#[test]
fn create_require_virtual_filenames_resolve_relative_packages_and_private_maps() {
    let project = Project::new();
    project.write("package.json", r##"{"imports":{"#value":"./value.cjs"}}"##);
    project.write("value.cjs", "module.exports = 42;");
    project.write(
        "node_modules/package/package.json",
        r#"{"exports":{"require":"./index.cjs"}}"#,
    );
    project.write("node_modules/package/index.cjs", "module.exports = 84;");
    project.succeeds("main.mjs", "import {createRequire} from 'rustyjs:module'; const base = import.meta.url.replace('/main.mjs','/missing/deep/virtual.cjs'); const req = createRequire(base); if (req('../../value.cjs') !== 42 || req('#value') !== 42 || req('package') !== 84) throw Error('virtual anchor'); const directory = createRequire(import.meta.url.replace('main.mjs','')); if (directory('./value.cjs') !== 42) throw Error('directory anchor'); console.log('virtual bases');", b"virtual bases\n");
    let native =
        serde_json::to_string(&project.0.join("missing/deep/native.cjs").to_string_lossy())
            .unwrap();
    project.succeeds("native.mjs", &format!("import {{createRequire}} from 'rustyjs:module'; const req = createRequire({native}); console.log(req('../../value.cjs'), req('package'));"), b"42 84\n");
}

#[test]
fn create_require_validates_strings_and_file_urls_without_guest_coercion() {
    let project = Project::new();
    project.succeeds("main.mjs", "import {createRequire} from 'rustyjs:module'; for (const base of [undefined,42,{},new String(import.meta.url),{get href(){throw Error('href getter');},toString(){throw Error('coerced');}},'./relative.cjs','https://example.invalid/a','file:///tmp/a?query','file:///tmp/a#hash','file:///tmp/a%2fb','file:///tmp/a%5cb','file:///tmp/a%00b',import.meta.url + '/child.cjs']) { try { createRequire(base); throw Error('invalid base accepted'); } catch(error) { if (!(error instanceof TypeError)) throw error; } } console.log('factory validation');", b"factory validation\n");
    project.succeeds("main.cjs", "const {createRequire} = require('rustyjs:module'); const req = createRequire(__filename); if (req.main !== module || req('rustyjs:module').createRequire !== createRequire) throw Error('CJS factory identity'); console.log('CJS factory');", b"CJS factory\n");
}

#[cfg(unix)]
#[test]
fn create_require_canonicalizes_symlink_ancestors_and_preserves_encoded_names() {
    let project = Project::new();
    project.write("space # café/value.cjs", "module.exports = 42;");
    std::os::unix::fs::symlink(project.0.join("space # café"), project.0.join("linked")).unwrap();
    project.succeeds("main.mjs", "import {createRequire} from 'rustyjs:module'; const physical = createRequire(import.meta.url.replace('main.mjs','space%20%23%20caf%C3%A9/virtual.cjs')); const linked = createRequire(import.meta.url.replace('main.mjs','linked/virtual.cjs')); if (physical.resolve('./value.cjs') !== linked.resolve('./value.cjs') || physical('./value.cjs') !== 42) throw Error('canonical factory base'); console.log('factory symlink');", b"factory symlink\n");
}
