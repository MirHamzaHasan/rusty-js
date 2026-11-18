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
