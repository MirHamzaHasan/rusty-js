mod common;

fn run(body: &str) -> std::process::Output {
    common::run(&[
        "-e",
        &format!("import('node:stream').then(async ({{Readable,Writable}})=>{{{body}}});"),
    ])
}
fn succeeds(body: &str, expected: &[u8]) {
    let output = run(body);
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
fn aliases_cjs_identity_inheritance_and_static_imports_are_shared() {
    succeeds(
        "const [a,b,c,{createRequire},{EventEmitter}]=await Promise.all([import('node:stream'),import('stream'),import('rustyjs:stream'),import('rustyjs:module'),import('events')]);const require=createRequire('/virtual.cjs');if(a!==b||b!==c||a.Readable!==Readable||require('stream').Readable!==Readable||require('node:stream')!==require('rustyjs:stream'))throw Error('identity');class Source extends Readable{_read(){this.push(null);}}const source=new Source();if(!(source instanceof EventEmitter)||source.readableFlowing!==null||source.readableHighWaterMark!==65536)throw Error('inheritance');source.resume();console.log('readable aliases');",
        b"readable aliases\n",
    );
    let path = format!("{}/examples/streams/pipe.mjs", env!("CARGO_MANIFEST_DIR"));
    let output = common::run(&[&path]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"hello world\n");
}

#[test]
fn read_sizes_split_join_and_preserve_shared_byte_views() {
    succeeds(
        "const source=new Readable({read(){}});const buffer=Buffer.from('abcd');source.push(buffer);source.push(new Uint8Array([101,102]));const first=source.read(2);if(first.toString()!=='ab'||source.readableLength!==4)throw Error('split');first[0]=65;if(buffer[0]!==65)throw Error('split sharing');if(source.read(3).toString()!=='cde'||source.readableLength!==1)throw Error('join');source.push(null);if(source.read(10).toString()!=='f'||source.read()!==null||source.readableHighWaterMark!==65536)throw Error('EOF short read');await Rusty.sleep(0);if(!source.readableEnded||!source.closed||!source.readableDidRead)throw Error('lifecycle');console.log('readable bytes');",
        b"readable bytes\n",
    );
}

#[test]
fn read_pulls_synchronously_and_prefetches_without_overlapping_hooks() {
    succeeds(
        "let calls=0,active=false;const source=new Readable({highWaterMark:2,read(size){if(size!==2||active)throw Error('overlapping pull');active=true;calls++;setTimeout(()=>{active=false;this.push(calls===3?null:'x');},0);}});if(source.read(2)!==null||calls!==1)throw Error('pull');const values=[];source.on('readable',()=>{let chunk;while((chunk=source.read(2))!==null)values.push(chunk.toString());});source.on('end',()=>{if(values.join(',')!=='xx'||calls!==3)throw Error('readable pulls');console.log('readable async pulls');});",
        b"readable async pulls\n",
    );
}

#[test]
fn flowing_pause_resume_and_listener_variants_keep_order() {
    succeeds(
        "const source=Readable.from(['a','b','c']);const values=[];let paused=false;source.prependOnceListener('data',value=>values.push('first:'+value));source.on('data',value=>{values.push(value);if(value==='a'){source.pause();paused=true;setTimeout(()=>{if(source.readableFlowing!==false||!source.isPaused())throw Error('pause state');source.resume();},0);}});source.on('end',()=>{if(!paused||values.join(',')!=='first:a,a,b,c')throw Error('flow order');console.log('readable flowing');});",
        b"readable flowing\n",
    );
}

#[test]
fn object_mode_preserves_falsy_values_and_read_zero_does_not_consume() {
    succeeds(
        "const values=[undefined,false,0,'',{}];const source=new Readable({objectMode:true,read(){}});for(const value of values)source.push(value);source.push(null);if(source.read(0)!==null||source.readableLength!==5)throw Error('read zero');for(const value of values)if(source.read(10)!==value)throw Error('object identity');if(source.read()!==null)throw Error('EOF');await Rusty.sleep(0);if(!source.readableEnded)throw Error('object end');console.log('readable object mode');",
        b"readable object mode\n",
    );
}

#[test]
fn high_water_mark_push_return_and_read_growth_are_advisory() {
    succeeds(
        "const source=new Readable({highWaterMark:2,read(){}});if(source.push('a')!==true||source.push('b')!==false||source.push('c')!==false||source.readableLength!==3)throw Error('push pressure');if(source.read(4)!==null||source.readableHighWaterMark!==4||source.readableLength!==3)throw Error('growth');source.push(null);if(source.read(4).toString()!=='abc')throw Error('EOF remainder');const zero=new Readable({highWaterMark:0,read(){this.push('x');this.push(null);}});if(zero.read().toString()!=='x')throw Error('zero highwater');console.log('readable pressure');",
        b"readable pressure\n",
    );
}

#[test]
fn unshift_before_end_delays_eof_until_the_new_bytes_are_consumed() {
    succeeds(
        "const source=new Readable({autoDestroy:false,read(){}});source.push('bc');source.push(null);source.unshift('a');if(source.read().toString()!=='abc')throw Error('unshift');source.unshift('x');await Rusty.sleep(0);if(source.readableEnded||source.read().toString()!=='x')throw Error('premature EOF');await Rusty.sleep(0);if(!source.readableEnded)throw Error('missing EOF');let reason;source.on('error',e=>reason=e);if(source.unshift('late')!==false)throw Error('late unshift');await Rusty.sleep(0);if(reason.code!=='ERR_STREAM_UNSHIFT_AFTER_END_EVENT')throw Error('late error');console.log('readable unshift');",
        b"readable unshift\n",
    );
}

#[test]
fn pipe_backpressure_waits_for_every_destination_and_ends_each_once() {
    succeeds(
        "const source=Readable.from(['a','b'],{highWaterMark:1});const pending=[[],[]],chunks=[[],[]],sinks=[];for(let i=0;i<2;i++)sinks[i]=new Writable({objectMode:true,highWaterMark:1,write(chunk,encoding,done){chunks[i].push(chunk);pending[i].push(done);}});if(source.pipe(sinks[0])!==sinks[0]||source.pipe(sinks[1])!==sinks[1])throw Error('pipe result');await Rusty.sleep(0);if(chunks[0].join(',')!=='a'||chunks[1].join(',')!=='a'||source.readableFlowing!==false)throw Error('initial blocked');pending[0].shift()();await Rusty.sleep(0);if(chunks[0].length!==1||chunks[1].length!==1)throw Error('resumed before all drains');pending[1].shift()();await Rusty.sleep(0);if(chunks[0].join(',')!=='a,b'||chunks[1].join(',')!=='a,b')throw Error('resume');pending[0].shift()();pending[1].shift()();await Rusty.sleep(0);for(const sink of sinks)if(!sink.writableFinished||!sink.closed||sink.listenerCount('drain')!==0||sink.listenerCount('error')!==0)throw Error('destination cleanup');if(source.listenerCount('data')!==0||!source.readableEnded)throw Error('source cleanup');console.log('readable pipe pressure');",
        b"readable pipe pressure\n",
    );
}

#[test]
fn unpipe_clears_blocked_destinations_without_resuming_a_manual_pause() {
    succeeds(
        "const source=Readable.from(['a','b'],{highWaterMark:1});let firstDone;const slow=new Writable({objectMode:true,highWaterMark:1,write(c,e,d){firstDone=d;}});const values=[];const fast=new Writable({objectMode:true,write(c,e,d){values.push(c);d();}});let unpiped=0;slow.on('unpipe',(s,info)=>{if(s!==source||!info.hasUnpiped)throw Error('unpipe metadata');unpiped++;});source.pipe(slow);source.pipe(fast);await Rusty.sleep(0);source.pause();source.unpipe(slow);firstDone();await Rusty.sleep(0);if(values.join(',')!=='a'||unpiped!==1)throw Error('manual pause lost');source.resume();await Rusty.sleep(0);if(values.join(',')!=='a,b'||!fast.writableFinished||slow.listenerCount('drain')!==0)throw Error('unpipe cleanup');slow.destroy();console.log('readable unpipe');",
        b"readable unpipe\n",
    );
}

#[test]
fn end_false_leaves_destination_open_and_undefined_objects_can_be_piped() {
    succeeds(
        "const source=Readable.from([undefined,0,false]);const values=[];const sink=new Writable({objectMode:true,write(c,e,d){values.push(c);d();}});source.pipe(sink,{end:false});await Rusty.sleep(0);if(!source.readableEnded||sink.writableEnded||values.length!==3||values[0]!==undefined||values[1]!==0||values[2]!==false||source.listenerCount('data')!==0)throw Error('end:false/object values');sink.end();console.log('readable end option');",
        b"readable end option\n",
    );
}

#[test]
fn destination_failures_unpipe_and_do_not_suppress_unhandled_errors() {
    succeeds(
        "const source=Readable.from(['a','b'],{highWaterMark:1});const sentinel=new Error('sink failed');let seen;const sink=new Writable({objectMode:true,write(c,e,d){d(sentinel);}});sink.on('error',e=>seen=e);source.pipe(sink);await Rusty.sleep(0);if(seen!==sentinel||source.destroyed||source.readableFlowing!==false||source.listenerCount('data')!==0||sink.listenerCount('drain')!==0||sink.listenerCount('error')!==1)throw Error('failure unlink');source.destroy();console.log('readable destination error');",
        b"readable destination error\n",
    );
    let output = run(
        "Readable.from(['x']).pipe(new Writable({objectMode:true,write(c,e,d){d(new Error('unhandled sink'));}}));",
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unhandled sink"));
}

#[test]
fn source_destroy_unpipes_and_custom_cleanup_cancels_host_timers() {
    succeeds(
        "let timer,cleanups=0,seen,unpipes=0;const failure=new Error('source failed');const source=new Readable({read(){timer=setTimeout(()=>this.push('late'),10000);},destroy(reason,done){cleanups++;clearTimeout(timer);done(reason);}});const sink=new Writable({write(c,e,d){d();}});source.on('error',e=>seen=e);sink.on('unpipe',()=>unpipes++);source.pipe(sink);await Rusty.sleep(0);source.destroy(failure);source.destroy();await Rusty.sleep(0);if(seen!==failure||cleanups!==1||unpipes!==1||!source.closed||!source.readableAborted||source.readableLength!==0||sink.writableEnded||sink.listenerCount('drain')!==0)throw Error('source destroy');if(source.push('late')!==false)throw Error('late push');sink.destroy();console.log('readable destruction');",
        b"readable destruction\n",
    );
}

#[test]
fn from_sync_async_iterables_promised_values_and_empty_byte_chunks() {
    succeeds(
        "const sources=[Readable.from('abc'),Readable.from(Buffer.from('def')),Readable.from(['',undefined,'é',''],{objectMode:false}),Readable.from((async function*(){yield Promise.resolve(1);await Rusty.sleep(0);yield 2;})())];const values=[[],[],[],[]];const finished=sources.map(source=>new Promise((resolve,reject)=>{source.once('end',resolve);source.once('error',reject);}));for(let i=0;i<sources.length;i++)sources[i].on('data',value=>values[i].push(value));await Promise.all(finished);if(values[0].join(',')!=='abc'||values[1][0].toString()!=='def'||values[2].map(x=>x.toString()).join(',')!=='é'||values[3].join(',')!=='1,2'||sources.some(s=>!s.readableEnded))throw Error('from values');console.log('readable iterables');",
        b"readable iterables\n",
    );
}

#[test]
fn from_destroy_calls_iterator_return_once_and_preserves_primary_failure() {
    succeeds(
        "let returns=0,pulls=0;const failure=new Error('consumer canceled');const iterable={[Symbol.asyncIterator](){return this;},next(){pulls++;return Promise.resolve({done:false,value:pulls});},return(){returns++;return Promise.reject(new Error('cleanup error'));}};const source=Readable.from(iterable,{highWaterMark:1});let seen;source.on('error',e=>seen=e);source.once('data',()=>source.destroy(failure));await Rusty.sleep(0);if(returns!==1||pulls>2||seen!==failure||!source.closed)throw Error('iterator cleanup');const nulls=Readable.from([null]);let nullError;nulls.on('error',e=>nullError=e);nulls.resume();await Rusty.sleep(0);if(nullError.code!=='ERR_STREAM_NULL_VALUES'||!nulls.closed)throw Error('null value');console.log('readable iterator cleanup');",
        b"readable iterator cleanup\n",
    );
}

#[test]
fn validation_eof_and_detached_queued_bytes_are_controlled() {
    succeeds(
        "for(const options of [null,{read:1},{encoding:'utf8'},{construct(){}},{signal:{}},{objectMode:1}]){try{new Readable(options);throw Error('bad options');}catch(e){if(!(e instanceof TypeError))throw e;}}const source=new Readable({read(){}});for(const action of [()=>source.push({}),()=>source.push('x','bad'),()=>source.read('1'),()=>source.pipe({}),()=>source.pipe(new Writable(),{end:1}),()=>Readable.from({})]){try{action();throw Error('invalid input');}catch(e){if(!(e instanceof TypeError))throw e;}}for(const value of [-1,1.5,Infinity,67108865]){try{source.read(value);throw Error('bad read size');}catch(e){if(!(e instanceof RangeError))throw e;}}const bytes=Buffer.from('x');source.push(bytes);bytes.buffer.transfer();let caught;source.on('error',e=>caught=e);if(source.read()!==null)throw Error('detached read');await Rusty.sleep(0);if(!(caught instanceof TypeError)||!source.closed)throw Error('detached error');const eof=new Readable({read(){}});let eofError;eof.on('error',e=>eofError=e);eof.push(null);eof.push('late');await Rusty.sleep(0);if(eofError.code!=='ERR_STREAM_PUSH_AFTER_EOF')throw Error('EOF error');console.log('readable validation');",
        b"readable validation\n",
    );
}

#[test]
fn read_hook_errors_and_listener_throws_do_not_skip_teardown() {
    let output = run("new Readable().resume();");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("_read is not implemented"));
    let output = run(
        "const source=Readable.from(['a']);source.on('end',()=>{throw Error('end listener threw');});source.on('close',()=>console.log('source closed'));source.resume();",
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("end listener threw"));
    assert_eq!(output.stdout, b"source closed\n");
}

#[test]
fn pipe_setup_rolls_back_when_metadata_listeners_throw_or_destroy_source() {
    succeeds(
        "for(const cancel of [false,true]){const source=new Readable({read(){}});const sink=new Writable({write(c,e,d){d();}});const sentinel=new Error('setup interrupted');source.on('newListener',name=>{if(name==='end'){if(cancel)source.destroy();else throw sentinel;}});try{source.pipe(sink);throw Error('setup succeeded');}catch(e){if(cancel?e.code!=='ERR_STREAM_DESTROYED':e!==sentinel)throw e;}if(source.listenerCount('data')!==0||source.listenerCount('end')!==0||sink.listenerCount('drain')!==0||sink.listenerCount('error')!==0)throw Error('partial pipe bindings');source.destroy();source.resume();if(source.readableFlowing!==false)throw Error('destroyed resumed');sink.destroy();}console.log('readable pipe rollback');",
        b"readable pipe rollback\n",
    );
}

#[test]
fn cleanup_continues_after_remove_listener_or_unpipe_handlers_throw() {
    let output = run(
        "const source=new Readable({read(){}});const sink=new Writable({write(c,e,d){d();}});source.pipe(sink);source.on('removeListener',name=>{if(name==='data')throw Error('remove metadata threw');});sink.on('unpipe',()=>{if(sink.listenerCount('drain')||sink.listenerCount('error')||source.listenerCount('data'))throw Error('bindings retained');console.log('unlinked');throw Error('unpipe listener threw');});source.on('close',()=>console.log('closed'));source.destroy();",
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("remove metadata threw"));
    assert_eq!(output.stdout, b"unlinked\nclosed\n");
}

#[test]
fn iterator_failures_late_results_and_constructor_validation_release_resources() {
    succeeds(
        "let acquired=0;const badOptions={[Symbol.iterator](){acquired++;return [][Symbol.iterator]();}};try{Readable.from(badOptions,{highWaterMark:-1});throw Error('options accepted');}catch(e){if(!(e instanceof RangeError)||acquired!==0)throw e;}for(const async of [false,true]){const failure=new Error('iterator failure');let returns=0,seen;const iterable={[Symbol.iterator](){return this;},next(){if(async)return Promise.reject(failure);throw failure;},return(){returns++;return {done:true};}};const source=Readable.from(iterable);source.on('error',e=>seen=e);source.resume();await Rusty.sleep(0);if(seen!==failure||returns!==1||!source.closed)throw Error('iterator error cleanup');}let settle,returned=0;const pending=Readable.from({[Symbol.iterator](){return this;},next(){return new Promise(resolve=>settle=resolve);},return(){returned++;return {done:true};}});let emitted=0;pending.on('data',()=>emitted++);await Rusty.sleep(0);pending.destroy();settle({done:false,value:42});await Rusty.sleep(0);if(returned!==1||emitted!==0||!pending.closed)throw Error('late result');console.log('readable iterator failures');",
        b"readable iterator failures\n",
    );
}

#[test]
fn byte_mode_pipe_conversion_errors_destroy_destination_and_detach() {
    succeeds(
        "const source=Readable.from([{}],{highWaterMark:1});let seen;const sink=new Writable({write(c,e,d){d();}});sink.on('error',e=>seen=e);source.pipe(sink);await Rusty.sleep(0);if(!(seen instanceof TypeError)||!sink.closed||source.listenerCount('data')!==0||sink.listenerCount('drain')!==0)throw Error('conversion failure');source.destroy();console.log('readable pipe conversion error');",
        b"readable pipe conversion error\n",
    );
}

#[test]
fn reentrant_destroy_during_unpipe_metadata_does_not_loop_or_retain_bindings() {
    succeeds(
        "const source=new Readable({read(){}});const sink=new Writable({write(c,e,d){d();}});let unpipes=0;source.pipe(sink);sink.on('unpipe',()=>unpipes++);source.on('removeListener',name=>{if(name==='data')source.destroy();});source.unpipe(sink);await Rusty.sleep(0);if(!source.closed||unpipes!==1||source.listenerCount('data')!==0||source.listenerCount('end')!==0||sink.listenerCount('drain')!==0||sink.listenerCount('error')!==0)throw Error('reentrant cleanup');sink.destroy();console.log('readable reentrant cleanup');",
        b"readable reentrant cleanup\n",
    );
}

#[test]
fn captured_intrinsics_and_event_key_conversion_keep_flowing_stable() {
    let output = common::run(&[
        "-e",
        "import('node:stream').then(({Readable})=>{Array.prototype.shift=()=>{throw Error('patched shift');};Array.prototype.indexOf=()=>{throw Error('patched indexOf');};WeakMap.prototype.get=()=>{throw Error('patched weak get');};Buffer.from=()=>{throw Error('patched Buffer.from');};Promise.prototype.then=()=>{throw Error('patched then');};let converted=0;const source=Readable.from(['a','b']);const values=[];source.on({[Symbol.toPrimitive](){converted++;return 'data';}},value=>values.push(value));source.on('end',()=>{if(converted!==1||values.join(',')!=='a,b')throw Error('captured flow');console.log('readable intrinsics');});});",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"readable intrinsics\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn internal_completion_jobs_do_not_assimilate_stream_then_properties() {
    succeeds(
        "let assimilated=0;const poison=()=>{assimilated++;throw Error('stream then invoked');};const source=Readable.from(['x']);source.then=poison;source.resume();const sink=new Writable({write(c,e,d){d();}});sink.then=poison;sink.end('x');const failure=new Readable({read(){throw Error('expected failure');}});failure.then=poison;failure.on('error',()=>{});failure.resume();await Rusty.sleep(0);if(assimilated||!source.closed||!sink.closed||!failure.closed)throw Error('completion assimilation');console.log('stream completion isolation');",
        b"stream completion isolation\n",
    );
}
