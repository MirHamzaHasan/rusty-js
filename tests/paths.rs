mod common;

fn succeeds(body: &str, expected: &[u8]) {
    let output = common::run(&[
        "-e",
        &format!("import('node:path').then(async (p)=>{{{body}}});"),
    ]);
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
fn aliases_variants_and_commonjs_share_original_objects_and_functions() {
    succeeds(
        r#"const [a,b,c,x,y,{createRequire}]=await Promise.all([import('node:path'),import('path'),import('rustyjs:path'),import('node:path/posix'),import('rustyjs:path/win32'),import('rustyjs:module')]);const require=createRequire('/virtual.cjs');if(a!==b||b!==c||a.default!==require('path')||a.posix!==x.default||a.win32!==y.default||require('path/posix')!==a.posix||require('node:path/win32')!==a.win32||a.posix.posix!==a.posix||a.win32.win32!==a.win32||a.win32.posix!==a.posix)throw Error('path identities');if(require.resolve('node:path')!=='node:path'||typeof globalThis.path!=='undefined')throw Error('resolution/global');const original=a.normalize;require('path').normalize=()=>{throw Error('replacement');};if((await import('path')).normalize!==original||a.normalize('a/../b')!=='b')throw Error('captured binding');console.log('path identities');"#,
        b"path identities\n",
    );
}

#[test]
fn posix_normalization_joining_and_absolute_rules_are_lexical() {
    succeeds(
        r#"const q=p.posix;for(const [input,expected]of [['','.'],['a/../../b','../b'],['/../../b','/b'],['a/../','./'],['//a///b/','/a/b/'],['a\\b','a\\b']])if(q.normalize(input)!==expected)throw Error('normalize '+input);if(q.join('/a','/b','..','c')!=='/a/c'||q.join('','','')!=='.'||q.join()!=='.'||!q.isAbsolute('/a')||q.isAbsolute('C:\\a')||q.sep!=='/'||q.delimiter!==':')throw Error('POSIX contract');console.log('posix paths');"#,
        b"posix paths\n",
    );
}

#[test]
fn parsing_suffixes_and_formatting_preserve_raw_separators_and_priority() {
    succeeds(
        r#"const q=p.posix;const parsed=q.parse('/a//archive.tar.gz/');if(JSON.stringify(parsed)!=='{"root":"/","dir":"/a/","base":"archive.tar.gz","ext":".gz","name":"archive.tar"}'||q.dirname('//file')!=='//'||q.dirname('a//b/')!=='a/'||q.basename('/foo/','foo')!=='foo'||q.basename('dir/foo','dir/foo')!==''||q.basename('archive.tar.gz','.gz')!=='archive.tar')throw Error('parts');for(const [input,ext]of [['.env',''],['.env.local','.local'],['...','.'],['..',''],['a.','.']])if(q.extname(input)!==ext)throw Error('extension');if(q.parse('/..').ext!=='.'||q.parse('//..').ext!=='')throw Error('parse compatibility');if(q.format({root:'/',dir:'/a',base:'b.js',name:'ignored',ext:'ignored'})!=='/a/b.js'||q.format({root:'/',name:'a',ext:'js'})!=='/a.js'||q.format({dir:'a/',base:'b'})!=='a//b')throw Error('format priority');console.log('path components');"#,
        b"path components\n",
    );
}

#[test]
fn windows_normalization_handles_drives_unc_and_colon_protection() {
    succeeds(
        r#"const q=p.win32;for(const [input,expected]of [['C:','C:.'],['C:./','C:.\\'],['C:/a/../b/','C:\\b\\'],['\\\\server\\share\\..\\x','\\\\server\\share\\x'],['a/../C:/x','.\\C:\\x'],['foo/xx:/..','.\\foo'],['foo:bar','foo:bar']])if(q.normalize(input)!==expected)throw Error('normalize '+input);if(q.join('/','/server','share')!=='\\server\\share'||q.join('//server','share','x')!=='\\\\server\\share\\x'||q.sep!=='\\'||q.delimiter!==';'||!q.isAbsolute('C:/a')||!q.isAbsolute('\\a')||q.isAbsolute('C:a'))throw Error('windows join/absolute');console.log('windows lexical paths');"#,
        b"windows lexical paths\n",
    );
}

#[test]
fn windows_parsing_and_relative_paths_keep_device_and_case_rules() {
    succeeds(
        r#"const q=p.win32;const parsed=q.parse('C:\\a\\file.TXT');if(parsed.root!=='C:\\'||parsed.dir!=='C:\\a'||parsed.name!=='file'||parsed.ext!=='.TXT'||q.basename('C:\\a\\file.TXT','.txt')!=='file.TXT')throw Error('windows parts');if(q.relative('C:\\A\\x','c:\\a\\y')!=='..\\y'||q.relative('C:\\a','D:\\b')!=='D:\\b'||q.relative('\\\\srv\\a\\x','\\\\srv\\b\\y')!=='..\\..\\b\\y')throw Error('windows relative');const unc=q.parse('//server/share/');if(unc.root!=='//server/share/'||unc.base!==''||q.basename('//server/share/')!=='share')throw Error('UNC root parts');console.log('windows path parts');"#,
        b"windows path parts\n",
    );
}

#[test]
fn namespaces_and_resolve_use_explicit_roots_and_host_cwd() {
    succeeds(
        r#"if(p.posix.resolve('/a','../b')!=='/b'||p.posix.relative('/a/b','/a/c')!=='../c'||p.posix.relative('/a','/a')!=='')throw Error('POSIX resolve');const current=p.resolve();if(!p.isAbsolute(current)||p.resolve('x')!==p.join(current,'x'))throw Error('cwd anchor');const q=p.win32;if(q.resolve('C:\\a','\\b')!=='C:\\b'||q.resolve('C:\\a','D:\\b')!=='D:\\b'||q.toNamespacedPath('C:\\a')!=='\\\\?\\C:\\a'||q.toNamespacedPath('\\\\server\\share\\a')!=='\\\\?\\UNC\\server\\share\\a')throw Error('windows resolve/namespace');const sentinel={toString(){throw Error('coercion');}};if(p.posix.toNamespacedPath(sentinel)!==sentinel||q.toNamespacedPath(sentinel)!==sentinel||q.toNamespacedPath('')!=='')throw Error('namespace passthrough');if(p.sep==='/'&&q.resolve('C:child')!=='C:'+q.resolve('child'))throw Error('drive-relative cwd');console.log('path resolution');"#,
        b"path resolution\n",
    );
}

#[test]
fn invalid_values_are_rejected_without_coercion_and_getter_errors_propagate() {
    succeeds(
        r#"for(const q of [p.posix,p.win32]){for(const value of [undefined,null,42,[],Symbol(),{toString(){throw Error('coerced');}}])for(const method of ['normalize','resolve','isAbsolute','join','dirname','basename','extname','parse']){try{q[method](value);throw Error('accepted '+method);}catch(e){if(!(e instanceof TypeError))throw e;}}for(const action of [()=>q.relative('a',{}),()=>q.basename('a',null),()=>q.format(null),()=>q.format([]),()=>q.format({name:1}),()=>q.format({dir:'a',base:{}}),()=>q.join('x',undefined),()=>q.resolve(null,'/absolute')]){try{action();throw Error('bad shape accepted');}catch(e){if(!(e instanceof TypeError))throw e;}}}const sentinel={};try{p.format({get dir(){throw sentinel;}});throw Error('getter ignored');}catch(e){if(e!==sentinel)throw e;}console.log('path validation');"#,
        b"path validation\n",
    );
}

#[test]
fn captured_intrinsics_and_prototype_safe_formatting_survive_guest_replacement() {
    succeeds(
        r#"String.prototype.slice=()=>{throw Error('patched slice');};String.prototype.split=()=>{throw Error('patched split');};String.prototype.toLowerCase=()=>{throw Error('patched lower');};String.prototype.lastIndexOf=()=>{throw Error('patched index');};Array.prototype.join=()=>{throw Error('patched join');};Array.prototype.pop=()=>{throw Error('patched pop');};Object.create=()=>{throw Error('patched create');};Object.defineProperty(Object.prototype,'dir',{set(){throw Error('inherited setter');},configurable:true});if(p.posix.normalize('/a/../b')!=='/b'||p.posix.format({root:'/',name:'x',ext:'js'})!=='/x.js'||p.win32.relative('C:\\A','c:\\b')!=='..\\b')throw Error('captured intrinsics');console.log('path intrinsics');"#,
        b"path intrinsics\n",
    );
}

#[test]
fn static_module_example_works_without_an_entrypoint_cwd_base() {
    let path = format!("{}/examples/path/main.mjs", env!("CARGO_MANIFEST_DIR"));
    let output = common::run(&[&path]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"app/main.js\nC:\\data\\rows.csv\nrows .csv\n"
    );
}

#[cfg(unix)]
fn temporary_directory(label: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "rustyjs-path-{label}-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn deleted_cwd_is_a_catchable_error_while_absolute_paths_still_work() {
    let directory = temporary_directory("deleted");
    let mut command = std::process::Command::new("/bin/sh");
    command.current_dir(&directory).arg("-c").arg("rmdir \"$1\" && shift && exec \"$@\"").arg("path-cwd-test").arg(&directory).arg(env!("CARGO_BIN_EXE_rustyjs")).arg("-e").arg(
        r#"import('node:path').then(p=>{if(p.posix.resolve('/a/../b')!=='/b'||p.win32.resolve('C:\\a')!=='C:\\a')throw Error('absolute accessed cwd');try{p.resolve('relative');throw Error('cwd failure ignored');}catch(e){if(!e.message.includes('cannot read current directory'))throw e;}console.log('cwd failure caught');});"#,
    );
    let output = common::run_command(command);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"cwd failure caught\n");
    assert!(!directory.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_cwd_is_rejected_without_lossy_replacement_or_host_panic() {
    use std::os::unix::ffi::OsStringExt;
    let root = temporary_directory("encoding");
    let directory = root.join(std::ffi::OsString::from_vec(vec![b'p', 0xff]));
    std::fs::create_dir(&directory).unwrap();
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_rustyjs"));
    command.current_dir(&directory).arg("-e").arg(r#"import('path').then(p=>{if(p.posix.resolve('/absolute')!=='/absolute')throw Error('absolute');try{p.resolve();throw Error('invalid cwd accepted');}catch(e){if(!e.message.includes('not valid UTF-8'))throw e;}console.log('cwd encoding caught');});"#);
    let output = common::run_command(command);
    std::fs::remove_dir(&directory).unwrap();
    std::fs::remove_dir(&root).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"cwd encoding caught\n");
}
