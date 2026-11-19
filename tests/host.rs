mod common;

fn succeeds(body: &str, expected: &[u8]) {
    let output = common::run(&["-e", body]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, expected);
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn process_os_aliases_and_commonjs_share_objects_with_captured_named_exports() {
    succeeds(
        r#"Promise.all([import('process'),import('node:process'),import('rustyjs:process'),import('os'),import('node:os'),import('rustyjs:os'),import('rustyjs:module')]).then(([a,b,c,x,y,z,{createRequire}])=>{const require=createRequire('/virtual.cjs');if(a!==b||b!==c||x!==y||y!==z||a.default!==process||require('process')!==process||require('node:process')!==process||require('os')!==x.default||require('rustyjs:os')!==x.default||a.env!==process.env||a.argv!==process.argv||a.hrtime!==process.hrtime)throw Error('identity');const original=a.cwd;process.cwd=()=>{throw Error('replacement');};if(a.cwd!==original||typeof a.cwd()!=='string')throw Error('named binding');console.log('host aliases');});"#,
        b"host aliases\n",
    );
}

#[test]
fn process_metadata_identifies_rustyjs_and_uses_runtime_clock() {
    succeeds(
        r#"if(process.argv[0]!==process.execPath||!process.execPath||!process.argv0||!Number.isInteger(process.pid)||process.pid<=0||process.version!=='v0.1.0'||process.versions.rustyjs!=='0.1.0'||!process.versions.v8||'node' in process.versions||process.release.name!=='rustyjs'||typeof process.exit!=='undefined'||typeof process.chdir!=='undefined')throw Error('metadata');if(!Object.isFrozen(process.versions)||Reflect.set(process,'platform','fake'))throw Error('immutable labels');const start=process.hrtime(),ns=process.hrtime.bigint(),uptime=process.uptime();Rusty.sleep(0).then(()=>{const diff=process.hrtime(start);if(typeof ns!=='bigint'||process.hrtime.bigint()<ns||process.uptime()<uptime||!Array.isArray(diff)||diff.length!==2||diff[0]<0||diff[1]<0||diff[1]>=1e9)throw Error('clock');for(const value of [null,{},[1],['1',0],[-1,0],[0,1e9]]){try{process.hrtime(value);throw Error('invalid clock argument');}catch(e){if(!(e instanceof TypeError)&&!(e instanceof RangeError))throw e;}}console.log('runtime metadata');});"#,
        b"runtime metadata\n",
    );
}

#[test]
fn os_facts_agree_with_process_and_native_paths() {
    let expected = serde_json::json!({"home":std::env::home_dir().unwrap().to_str().unwrap(),"temp":std::env::temp_dir().to_str().unwrap().trim_end_matches(std::path::MAIN_SEPARATOR)});
    let body = format!("const expected={expected};")
        + r#"import('node:os').then(({default:os})=>{if(os.platform()!==process.platform||os.arch()!==process.arch||!['LE','BE'].includes(os.endianness())||!Number.isInteger(os.availableParallelism())||os.availableParallelism()<1||os.homedir()!==expected.home||os.tmpdir()!==expected.temp)throw Error('host facts');if(process.platform==='darwin'&&(os.type()!=='Darwin'||os.EOL!=='\n'||os.devNull!=='/dev/null'))throw Error('Darwin facts');console.log('os facts');});"#;
    succeeds(&body, b"os facts\n");
}

#[test]
fn cli_passes_eval_arguments_and_runtime_flags_separately() {
    let source = r#"if(JSON.stringify(process.argv.slice(1))!=='["a","--flag","é"]'||process.execArgv[0]!=='-e'||process.execArgv[1].indexOf('process.argv')<0)throw Error('eval argv');console.log('eval arguments');"#;
    let output = common::run(&["-e", source, "--", "a", "--flag", "é"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"eval arguments\n");
}

#[test]
fn cli_file_modes_pass_absolute_entrypoint_and_preserve_script_flags() {
    let path = format!("{}/tests/fixtures/host-args.js", env!("CARGO_MANIFEST_DIR"));
    for mode in [None, Some("--module"), Some("--commonjs")] {
        let mut args = Vec::new();
        if let Some(mode) = mode {
            args.push(mode);
        }
        args.extend([path.as_str(), "--", "--module", "space value"]);
        let output = common::run(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let data: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            data["args"],
            serde_json::json!([path, "--", "--module", "space value"])
        );
        assert_eq!(
            data["flags"],
            mode.map_or(serde_json::json!([]), |mode| serde_json::json!([mode]))
        );
    }
}

#[test]
fn environment_is_inherited_by_cli_but_edits_are_local_and_prototype_safe() {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_rustyjs"));
    command.env("RUSTYJS_TEST_ENV","original").arg("-e").arg(r#"if(process.env.RUSTYJS_TEST_ENV!=='original'||Object.getPrototypeOf(process.env)!==null)throw Error('environment inheritance');process.env.RUSTYJS_TEST_ENV=42;if(process.env.RUSTYJS_TEST_ENV!=='42')throw Error('string conversion');process.env.__proto__='value';if(process.env.__proto__!=='value'||Object.getPrototypeOf(process.env)!==null)throw Error('prototype');delete process.env.RUSTYJS_TEST_ENV;if('RUSTYJS_TEST_ENV' in process.env)throw Error('delete');for(const action of [()=>{process.env['bad=name']='x';},()=>{process.env['']='x';},()=>{process.env[Symbol()]='x';},()=>{process.env.X=Symbol();},()=>Object.freeze(process.env),()=>Object.defineProperty(process.env,'X',{value:'y'})]){try{action();throw Error('invalid environment mutation');}catch(e){if(!(e instanceof TypeError))throw e;}}const sentinel={};try{process.env.X={toString(){throw sentinel;}};throw Error('coercion ignored');}catch(e){if(e!==sentinel)throw e;}console.log('environment view');"#);
    let output = common::run_command(command);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"environment view\n");
}

#[test]
fn library_defaults_do_not_inherit_environment_and_options_remain_runtime_local() {
    let before = std::env::var_os("RUSTYJS_EMBED_ONLY");
    {
        let mut js = rustyjs::JsRuntime::new().unwrap();
        js.execute("if(Object.keys(process.env).length||process.argv.length!==1||process.execArgv.length)throw Error('ambient inheritance');process.env.RUSTYJS_EMBED_ONLY='local';process.exitCode=9;","default.js").unwrap();
        assert_eq!(js.exit_code(), Some(9));
    }
    let mut options = rustyjs::RuntimeOptions {
        args: vec!["entry.js".into(), "one".into()],
        exec_args: vec!["--embedded".into()],
        argv0: Some("host-label".into()),
        ..Default::default()
    };
    options
        .env
        .insert("RUSTYJS_EMBED_ONLY".into(), "explicit".into());
    let mut js = rustyjs::JsRuntime::with_options(options).unwrap();
    js.execute("if(process.env.RUSTYJS_EMBED_ONLY!=='explicit'||process.argv[1]!=='entry.js'||process.argv[2]!=='one'||process.argv0!=='host-label'||process.execArgv[0]!=='--embedded'||process.exitCode!==undefined)throw Error('explicit context');process.exitCode=4;throw Error('recover');","configured.js").unwrap_err();
    js.execute("process.exitCode=undefined;if(process.env.RUSTYJS_EMBED_ONLY!=='explicit')throw Error('context lost');","reuse.js").unwrap();
    assert_eq!(js.exit_code(), None);
    assert_eq!(std::env::var_os("RUSTYJS_EMBED_ONLY"), before);
}

#[test]
fn cli_exit_code_waits_for_async_work_and_errors_override_requested_status() {
    let output = common::run(&[
        "-e",
        "process.exitCode=7;Rusty.sleep(0).then(()=>console.log('drained'));globalThis.process={exitCode:0};",
    ]);
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(output.stdout, b"drained\n");
    assert!(output.stderr.is_empty());
    for code in [0, 7] {
        let output = common::run(&[
            "-e",
            &format!("process.exitCode={code};throw Error('guest failure');"),
        ]);
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("guest failure"));
    }
    succeeds(
        "for(const value of [-1,256,NaN,Infinity,1.5]){try{process.exitCode=value;throw Error('bad code');}catch(e){if(!(e instanceof RangeError))throw e;}}for(const value of ['1',null,{}]){try{process.exitCode=value;throw Error('bad type');}catch(e){if(!(e instanceof TypeError))throw e;}}process.exitCode=3;process.exitCode=undefined;console.log('exit validation');",
        b"exit validation\n",
    );
}

#[test]
fn global_replacement_does_not_replace_captured_process_exports_or_native_exit_state() {
    succeeds(
        "const original=process;globalThis.process=null;import('node:process').then(p=>{if(p.default!==original||p.argv!==original.argv||p.cwd!==original.cwd)throw Error('global lookup');p.default.exitCode=0;console.log('captured process');});",
        b"captured process\n",
    );
}

#[test]
fn invalid_embedder_environment_names_return_errors_without_exposing_values() {
    let mut options = rustyjs::RuntimeOptions::default();
    options
        .env
        .insert("bad=name".into(), "private-value".into());
    let error = match rustyjs::JsRuntime::with_options(options) {
        Ok(_) => panic!("bad configuration accepted"),
        Err(error) => error,
    };
    assert!(error.message.contains("invalid environment variable name"));
    assert!(!error.message.contains("private-value"));
}

#[cfg(unix)]
#[test]
fn non_utf8_arguments_fail_and_non_utf8_environment_values_are_skipped() {
    use std::os::unix::ffi::OsStringExt;
    let invalid = std::ffi::OsString::from_vec(vec![0xff]);
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_rustyjs"));
    command
        .arg("-e")
        .arg("throw Error('should not run')")
        .arg(&invalid);
    let output = common::run_command(command);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("script arguments must be valid UTF-8")
    );
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_rustyjs"));
    command.env("RUSTYJS_INVALID_UNICODE",invalid).arg("-e").arg("if('RUSTYJS_INVALID_UNICODE' in process.env)throw Error('lossy environment');console.log('encoding policy');");
    let output = common::run_command(command);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"encoding policy\n");
}
