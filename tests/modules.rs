mod common;
use common::run;

fn fixture(name: &str) -> String {
    format!(
        "{}/tests/fixtures/modules/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn succeeds(name: &str, expected: &[u8]) {
    let output = run(&[&fixture(name)]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, expected);
    assert!(output.stderr.is_empty());
}

#[test]
fn canonical_graph_identity_live_bindings_and_reexports() {
    succeeds("graph.mjs", b"graph 1 2\n");
}

#[test]
fn cyclic_module_graphs_link_and_evaluate() {
    succeeds("cycle.mjs", b"AB AB\n");
}

#[test]
fn top_level_await_drives_transitive_dependencies_and_timers() {
    succeeds("await-main.mjs", b"awaited 42\n");
}

#[test]
fn explicit_module_mode_preserves_classic_js_default() {
    let path = fixture("as-js.js");
    let output = run(&["--module", &path]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"explicit 42\n");
    assert_eq!(run(&[&path]).status.code(), Some(1));
}

#[test]
fn module_preparation_and_link_errors_are_controlled_diagnostics() {
    for (name, message) in [
        ("bare.mjs", "unsupported module specifier"),
        ("attributes.mjs", "import attributes"),
        ("missing.mjs", "does-not-exist.mjs"),
        ("syntax.mjs", "SyntaxError"),
        ("missing-export.mjs", "doesNotExist"),
    ] {
        let output = run(&[&fixture(name)]);
        assert_eq!(output.status.code(), Some(1));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains(name), "{error}");
        assert!(error.contains(message), "{error}");
        assert!(!error.contains("panicked"), "{error}");
    }
}

#[test]
fn idle_top_level_await_is_an_error_instead_of_silent_success() {
    let output = run(&[&fixture("pending.mjs")]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("pending with no active host work"));
}

#[test]
fn rejected_evaluation_cancels_remaining_host_work() {
    let output = run(&[&fixture("rejected.mjs")]);
    assert_eq!(output.status.code(), Some(1));
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("rejected.mjs"), "{error}");
    assert!(error.contains("module-failed"), "{error}");
    assert!(!error.contains("panicked"), "{error}");
}

#[test]
fn dynamic_import_reuses_namespaces_and_awaits_host_work() {
    succeeds("dynamic.mjs", b"dynamic 2 42 42\n");
}

#[test]
fn dynamic_import_errors_are_catchable_and_preserve_guest_values() {
    succeeds("dynamic-errors.mjs", b"caught dynamic failures\n");
}

#[test]
fn classic_files_can_import_modules() {
    succeeds("dynamic-classic.js", b"classic dynamic 1\n");
}

#[test]
fn idle_dynamic_evaluation_rejects_without_hanging() {
    succeeds("dynamic-idle.mjs", b"caught idle import\n");
    succeeds(
        "dynamic-idle-chain.mjs",
        b"caught idle import\nrecovered chain\n",
    );
}

#[test]
fn uncaught_import_failure_is_reported_and_inline_import_has_no_relative_base() {
    for source in ["import('./missing.mjs')", "import('node:fs')"] {
        let output = run(&["-e", source]);
        assert_eq!(output.status.code(), Some(1));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("Unhandled promise rejection"), "{error}");
        assert!(!error.contains("panicked"), "{error}");
    }
}
