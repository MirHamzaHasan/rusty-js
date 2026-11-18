mod common;

fn run(body: &str) -> std::process::Output {
    common::run(&[
        "-e",
        &format!("import('node:stream').then(async ({{Writable}})=>{{{body}}});"),
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
fn aliases_commonjs_identity_and_event_inheritance_are_shared() {
    succeeds(
        "const [a,b,c,{createRequire},{EventEmitter}]=await Promise.all([import('node:stream'),import('stream'),import('rustyjs:stream'),import('rustyjs:module'),import('events')]);const require=createRequire('/virtual.cjs');if(a!==b||b!==c||a.Writable!==Writable||require('stream').Writable!==Writable||require('stream')!==require('node:stream')||require('rustyjs:stream')!==require('stream'))throw Error('identity');class Sink extends Writable{_write(chunk,encoding,done){done();}}const sink=new Sink();if(!(sink instanceof EventEmitter)||!sink.writable||sink.writableHighWaterMark!==65536)throw Error('inheritance');sink.end();console.log('stream aliases');",
        b"stream aliases\n",
    );
}

#[test]
fn synchronous_hooks_have_deferred_callbacks_finish_and_close() {
    succeeds(
        "const order=[];const sink=new Writable({write(chunk,encoding,done){if(this!==sink||!Buffer.isBuffer(chunk)||encoding!=='buffer')throw Error('write arguments');order.push('hook:'+chunk);done();},final(done){order.push('final');done();}});sink.on('prefinish',()=>order.push('prefinish')).on('finish',()=>{if(!sink.writableFinished)throw Error('finished flag');order.push('finish');}).on('close',()=>{if(!sink.closed||!sink.destroyed||sink.writableAborted)throw Error('close flags');order.push('close');console.log(order.join(','));});sink.write('a',function(){'use strict';if(this!==undefined)throw Error('callback receiver');order.push('callback');});sink.end(()=>order.push('end callback'));order.push('returned');if(order.join(',')!=='hook:a,returned'||!sink.writableEnded||sink.writable)throw Error('synchronous completion');",
        b"hook:a,returned,callback,final,prefinish,finish,end callback,close\n",
    );
}

#[test]
fn backpressure_counts_inflight_and_queued_bytes_until_drain() {
    succeeds(
        "const pending=[],chunks=[],order=[];const sink=new Writable({highWaterMark:3,write(chunk,encoding,done){chunks.push(chunk.toString());pending.push(done);}});sink.on('drain',()=>order.push('drain'));if(!sink.write('a',()=>order.push('a callback'))||sink.write('bc',()=>order.push('bc callback'))!==false||sink.writableLength!==3||!sink.writableNeedDrain||chunks.join(',')!=='a')throw Error('backpressure');pending.shift()();await Rusty.sleep(0);if(sink.writableLength!==2||chunks.join(',')!=='a,bc'||order.join(',')!=='a callback')throw Error('queue advance');pending.shift()();await Rusty.sleep(0);if(sink.writableLength!==0||sink.writableNeedDrain||order.join(',')!=='a callback,drain,bc callback')throw Error('drain');sink.end();console.log('writable backpressure');",
        b"writable backpressure\n",
    );
}

#[test]
fn cork_nesting_serialization_and_end_flush_are_deterministic() {
    succeeds(
        "const chunks=[];const sink=new Writable({write(chunk,encoding,done){chunks.push(chunk.toString());done();}});sink.cork();sink.cork();sink.write('a');sink.write('b');if(sink.writableCorked!==2||chunks.length)throw Error('cork');sink.uncork();if(chunks.length)throw Error('nested cork');sink.end('c',()=>{if(chunks.join(',')!=='a,b,c'||sink.writableCorked!==0)throw Error('flush');console.log('corked writes');});",
        b"corked writes\n",
    );
}

#[test]
fn object_mode_and_decode_strings_false_preserve_values() {
    succeeds(
        "const value={},seen=[];const objectSink=new Writable({objectMode:true,highWaterMark:1,write(chunk,encoding,done){seen.push(chunk);done();}});if(objectSink.write(value)!==false||objectSink.writableLength!==1||!objectSink.writableObjectMode)throw Error('object mode');objectSink.end();const raw=new Writable({decodeStrings:false,defaultEncoding:'latin1',write(chunk,encoding,done){if(chunk!=='é'||encoding!=='latin1')throw Error('raw string');done();}});raw.end('é');await Rusty.sleep(0);if(seen[0]!==value||!objectSink.writableFinished||!raw.writableFinished)throw Error('completion');console.log('object and string modes');",
        b"object and string modes\n",
    );
}

#[test]
fn byte_chunks_and_default_encoding_use_buffer_contract() {
    succeeds(
        "const chunks=[],encodings=[],buffer=Buffer.from([1]);const sink=new Writable({write(chunk,encoding,done){chunks.push(chunk);encodings.push(encoding);done();}});sink.setDefaultEncoding('hex');sink.write('ff');sink.write(buffer);sink.end(new Uint8Array([2]),()=>{if(chunks[0].toString('hex')!=='ff'||chunks[1]!==buffer||chunks[2].toString('hex')!=='02'||encodings.join(',')!=='buffer,buffer,buffer')throw Error('conversion');console.log('stream byte conversion');});",
        b"stream byte conversion\n",
    );
}

#[test]
fn write_errors_fail_all_callbacks_once_and_close_without_finish() {
    succeeds(
        "const failure=new Error('sink failed');let complete;const order=[];const sink=new Writable({write(chunk,encoding,done){complete=done;}});sink.on('error',error=>{if(error!==failure)throw Error('identity');order.push('error');}).on('finish',()=>{throw Error('finished after failure');}).on('close',()=>{if(!sink.writableAborted||sink.errored!==failure||sink.writableLength!==0)throw Error('failed state');if(order.join(',')!=='a,b,end,error')throw Error('failure order '+order);console.log('stream failure cleanup');});for(const chunk of ['a','b'])sink.write(chunk,error=>{if(error!==failure)throw Error('callback error');order.push(chunk);});sink.end(error=>{if(error!==failure)throw Error('end error');order.push('end');});complete(failure);complete();",
        b"stream failure cleanup\n",
    );
}

#[test]
fn destroy_cancels_queued_writes_and_ignores_late_completions() {
    succeeds(
        "let callback,timer,calls=0,cleanups=0,closed=0;const sink=new Writable({write(chunk,encoding,done){callback=done;timer=setTimeout(()=>done(),10000);},destroy(error,done){cleanups++;clearTimeout(timer);done(error);}});sink.on('close',()=>closed++);sink.write('a',error=>{if(error.code!=='ERR_STREAM_DESTROYED')throw Error('destroy error');calls++;});sink.write('b',()=>calls++);sink.destroy();sink.destroy(new Error('ignored'));callback();await Rusty.sleep(0);if(calls!==2||cleanups!==1||closed!==1||!sink.destroyed||!sink.closed||sink.writableLength!==0)throw Error('destroy state');console.log('stream destruction');",
        b"stream destruction\n",
    );
}

#[test]
fn async_write_and_final_hooks_run_on_existing_host_timers() {
    succeeds(
        "const order=[];const sink=new Writable({write(chunk,encoding,done){setTimeout(()=>{order.push(chunk.toString());done();},0);},final(done){setTimeout(()=>{order.push('final');done();},0);}});sink.write('a');sink.write('b');sink.end(()=>{if(order.join(',')!=='a,b,final')throw Error('async order');console.log('stream async hooks');});",
        b"stream async hooks\n",
    );
}

#[test]
fn final_errors_missing_hooks_and_write_after_end_are_controlled() {
    succeeds(
        "const cases=[new Writable(),new Writable({write(c,e,d){d();},final(done){done(new Error('final failed'));}}),new Writable({write(c,e,d){d();}})];const errors=[];for(let i=0;i<cases.length;i++){cases[i].on('error',e=>errors.push(e));cases[i].on('finish',()=>{throw Error('unexpected finish');});}cases[0].end('x');cases[1].end();cases[2].end();if(cases[2].write('late')!==false)throw Error('write after end');await Rusty.sleep(0);if(errors.length!==3||!errors.some(e=>e.code==='ERR_STREAM_WRITE_AFTER_END')||!errors.some(e=>e.code==='ERR_METHOD_NOT_IMPLEMENTED')||!errors.some(e=>e.message==='final failed'))throw Error('controlled errors');console.log('stream execution errors');",
        b"stream execution errors\n",
    );
}

#[test]
fn duplicate_callbacks_and_thrown_hooks_destroy_once() {
    succeeds(
        "const errors=[];const duplicate=new Writable({write(chunk,encoding,done){done();done();}});duplicate.on('error',e=>errors.push(e));duplicate.end('a');const sentinel=new Error('thrown hook');const thrown=new Writable({write(){throw sentinel;}});thrown.on('error',e=>errors.push(e));thrown.end('x');await Rusty.sleep(0);if(errors.length!==2||!errors.some(e=>e.code==='ERR_MULTIPLE_CALLBACK')||!errors.includes(sentinel)||!duplicate.closed||!thrown.closed)throw Error('hook errors');console.log('stream hook validation');",
        b"stream hook validation\n",
    );
}

#[test]
fn validation_rejects_invalid_options_chunks_callbacks_and_detached_buffers() {
    succeeds(
        "for(const options of [{highWaterMark:-1},{highWaterMark:1.5},{highWaterMark:67108865}]){try{new Writable(options);throw Error('bad bounds');}catch(e){if(!(e instanceof RangeError))throw e;}}for(const options of [null,{objectMode:1},{highWaterMark:'1'},{write:1},{signal:{}},{construct(){}},{writev(){}}]){try{new Writable(options);throw Error('bad option');}catch(e){if(!(e instanceof TypeError))throw e;}}const sink=new Writable({write(c,e,d){d();}});const detached=Buffer.from([1]);detached.buffer.transfer();for(const action of [()=>sink.write(null),()=>sink.write({}),()=>sink.write('x','bad'),()=>sink.write('x','utf8',42),()=>sink.end('x','utf8',42),()=>sink.write(detached),()=>Writable.prototype.write.call({},'x')]){try{action();throw Error('bad input');}catch(e){if(!(e instanceof TypeError))throw e;}}if(sink.writableLength!==0)throw Error('validation mutated queue');sink.end();console.log('stream validation');",
        b"stream validation\n",
    );
}

#[test]
fn unhandled_errors_fail_cli_and_listener_throws_do_not_skip_cleanup() {
    let output = run("new Writable().end('x');");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("_write is not implemented"));
    let output = run(
        "const sink=new Writable({write(c,e,d){d();}});sink.on('finish',()=>{throw Error('finish listener');});sink.on('close',()=>console.log('cleaned up'));sink.end();",
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("finish listener"));
    assert_eq!(output.stdout, b"cleaned up\n");
}

#[test]
fn drain_reentry_waits_for_active_callback_and_preserves_queue_order() {
    succeeds(
        "const chunks=[],callbacks=[];let drains=0;const sink=new Writable({highWaterMark:1,write(chunk,encoding,done){chunks.push(chunk.toString());done();}});sink.on('drain',()=>{drains++;if(drains===1)sink.write('b',()=>{callbacks.push('b');sink.end();});});sink.on('close',()=>{if(chunks.join(',')!=='a,b'||callbacks.join(',')!=='a,b'||drains!==2)throw Error('reentrant drain');console.log('stream drain reentry');});sink.write('a',()=>callbacks.push('a'));",
        b"stream drain reentry\n",
    );
}

#[test]
fn lifecycle_options_repeated_end_and_cleanup_hook_errors_are_observable() {
    succeeds(
        "let ends=0,closes=0;const sink=new Writable({autoDestroy:false,emitClose:false,write(c,e,d){d();}});sink.on('close',()=>closes++);sink.end(()=>ends++);sink.end(()=>ends++);await Rusty.sleep(0);sink.end(()=>ends++);await Rusty.sleep(0);if(ends!==3||!sink.writableFinished||sink.destroyed||sink.closed)throw Error('repeat end');sink.destroy();await Rusty.sleep(0);if(!sink.closed||closes!==0)throw Error('emitClose');const failure=new Error('cleanup threw');let seen;const broken=new Writable({destroy(reason,done){done();throw failure;}});broken.on('error',e=>seen=e);broken.destroy();await Rusty.sleep(0);if(seen!==failure||!broken.closed)throw Error('cleanup throw lost');console.log('stream lifecycle options');",
        b"stream lifecycle options\n",
    );
}

#[test]
fn callbacks_throwing_do_not_lose_later_writes_or_finish() {
    let output = run(
        "const chunks=[];const sink=new Writable({write(c,e,d){chunks.push(c.toString());d();}});sink.write('a',()=>{throw Error('write callback threw');});sink.end('b');sink.on('close',()=>console.log(chunks.join(',')));",
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("write callback threw"));
    assert_eq!(output.stdout, b"a,b\n");
}
