mod common;
use common::run;

#[test]
fn evaluates_inline_and_logs_values() {
    let output = run(&[
        "-e",
        "console.log('answer', 6 * 7, null, undefined); console.log();",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"answer 42 null undefined\n\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn executes_a_file() {
    let path = format!("{}/examples/hello.js", env!("CARGO_MANIFEST_DIR"));
    let output = run(&[&path]);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"Hello from RustyJS 42\n");
}

#[test]
fn reports_guest_errors_without_host_panics() {
    for source in [
        "function nope( {",
        "function fail() { throw Error('broken'); } fail();",
        "console.log({ toString() { throw Error('conversion'); } });",
        "throw Object.create(null)",
    ] {
        let output = run(&["-e", source]);
        assert_eq!(output.status.code(), Some(1));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("<eval>"), "{error}");
        assert!(!error.contains("panicked"), "{error}");
    }
}

#[test]
fn validates_arguments_and_missing_files() {
    for args in [
        vec![],
        vec!["-e"],
        vec!["--module"],
        vec!["--unknown"],
        vec!["--"],
        vec!["--help", "extra"],
        vec!["/does-not-exist/rustyjs.js"],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(1));
        assert!(!output.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    }
}

#[test]
fn shows_help_and_version() {
    assert!(String::from_utf8_lossy(&run(&["--help"]).stdout).contains("Usage:"));
    let output = run(&["--version"]);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"rustyjs 0.1.0\n");
}

#[test]
fn drains_chained_microtasks_before_exit() {
    let output = run(&[
        "-e",
        r#"
        console.log('start');
        Promise.resolve().then(() => {
            console.log('first');
            Promise.resolve().then(() => console.log('nested'));
        }).then(() => console.log('chain'));
        console.log('end');
    "#,
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"start\nend\nfirst\nnested\nchain\n");
}

#[test]
fn drives_native_promises_and_work_created_by_reactions() {
    let output = run(&[
        "-e",
        r#"
        console.log('start');
        Rusty.sleep(0).then(value => {
            if (value !== undefined) throw Error('wrong result');
            console.log('timer');
            return Rusty.sleep(0);
        }).then(() => console.log('nested timer'));
        Promise.resolve().then(() => console.log('microtask'));
        console.log('end');
    "#,
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"start\nend\nmicrotask\ntimer\nnested timer\n"
    );
}

#[test]
fn validates_sleep_without_coercing_objects() {
    let output = run(&[
        "-e",
        r#"
        for (const value of [undefined, null, '1', NaN, Infinity, -1, 0.5, 2147483648,
            { valueOf() { throw Error('must not coerce'); } }]) {
            try { Rusty.sleep(value); throw Error('accepted invalid delay'); }
            catch (error) { if (!(error instanceof TypeError)) throw error; }
        }
        console.log('validated');
    "#,
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"validated\n");
}

#[test]
fn reports_unhandled_rejections_and_async_callback_errors() {
    for source in [
        "Promise.reject(Error('rejected'))",
        "Promise.resolve().then(() => { throw Error('reaction'); })",
        "Rusty.sleep(0).then(() => { throw Error('async reaction'); })",
        "Promise.reject(Object.create(null))",
        "Promise.reject({ toString() { throw Error('hostile'); } })",
    ] {
        let output = run(&["-e", source]);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("Unhandled promise rejection"), "{error}");
        assert!(!error.contains("panicked"), "{error}");
    }
}

#[test]
fn permits_rejections_handled_later_in_the_same_checkpoint() {
    let output = run(&[
        "-e",
        r#"
        const rejected = Promise.reject(Error('handled'));
        Promise.resolve().then(() => rejected.catch(() => console.log('caught')));
        new Promise(() => {});
    "#,
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"caught\n");
}

#[test]
fn timers_preserve_arguments_receiver_and_microtask_order() {
    let output = run(&[
        "-e",
        r#"
        const object = {};
        console.log('script');
        setTimeout(function(value, label) {
            console.log(value === object, label, this === globalThis);
            Promise.resolve().then(() => console.log('microtask'));
        }, undefined, object, 'argument');
        setTimeout(() => console.log('second'));
    "#,
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"script\ntrue argument true\nmicrotask\nsecond\n"
    );
}

#[test]
fn clearing_queued_and_ready_timers_prevents_callbacks() {
    let output = run(&[
        "-e",
        r#"
        const timeout = setTimeout(() => { throw Error('cleared timeout ran'); }, 2147483647);
        const interval = setInterval(() => { throw Error('cleared interval ran'); }, 2147483647);
        if (!Number.isSafeInteger(timeout) || !Number.isSafeInteger(interval) || timeout === interval) throw Error('invalid IDs');
        clearInterval(timeout);
        clearTimeout(interval);
        clearTimeout(timeout);
        for (const id of [undefined, null, NaN, Infinity, -1, 0.5, '1', {}]) clearTimeout(id);
        setTimeout(() => clearTimeout(ready), 0);
        const ready = setTimeout(() => { throw Error('ready cancelled callback ran'); }, 0);
        console.log('cleared');
    "#,
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"cleared\n");
}

#[test]
fn intervals_can_clear_themselves_inside_callbacks_and_microtasks() {
    let output = run(&[
        "-e",
        r#"
        let count = 0;
        const interval = setInterval((label) => {
            console.log(label, ++count);
            if (count === 3) clearInterval(interval);
        }, 0, 'tick');
        let once = 0;
        const clearedInMicrotask = setInterval(() => {
            if (++once !== 1) throw Error('interval rearmed after clear');
            Promise.resolve().then(() => clearTimeout(clearedInMicrotask));
        });
    "#,
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"tick 1\ntick 2\ntick 3\n");
}

#[test]
fn timer_validation_never_coerces_callbacks_or_delays() {
    let output = run(&[
        "-e",
        r#"
        for (const create of [setTimeout, setInterval]) {
            for (const callback of [undefined, null, 'console.log(1)', 42, {}]) {
                try { create(callback); throw Error('invalid callback accepted'); }
                catch (error) { if (!(error instanceof TypeError)) throw error; }
            }
            for (const delay of [null, '1', NaN, Infinity, -1, 0.5, 2147483648,
                { valueOf() { throw Error('must not coerce'); } }]) {
                try { create(() => {}, delay); throw Error('invalid delay accepted'); }
                catch (error) { if (!(error instanceof TypeError)) throw error; }
            }
        }
        console.log('validated');
    "#,
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"validated\n");
}

#[test]
fn timer_exceptions_and_rejections_fail_without_waiting_for_other_timers() {
    for callback in [
        "() => { throw Error('timer failed'); }",
        "async () => { throw Error('async timer failed'); }",
    ] {
        let source = format!("setTimeout({callback}, 0); setInterval(() => {{}}, 2147483647);");
        let output = run(&["-e", &source]);
        assert_eq!(output.status.code(), Some(1));
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("<eval>"), "{error}");
        assert!(error.contains("timer failed"), "{error}");
        assert!(!error.contains("panicked"), "{error}");
    }
}
