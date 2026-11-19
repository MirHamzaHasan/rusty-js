mod common;

fn run(body: &str) -> std::process::Output {
    common::run(&[
        "-e",
        &format!(
            "import('node:stream').then(async ({{Readable,Writable,finished,pipeline,promises}})=>{{{body}}});"
        ),
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
fn promise_modules_and_commonjs_share_original_exports_and_object() {
    succeeds(
        "const [a,b,c,{createRequire}]=await Promise.all([import('node:stream/promises'),import('stream/promises'),import('rustyjs:stream/promises'),import('rustyjs:module')]);const require=createRequire('/virtual.cjs');if(a!==b||b!==c||a.pipeline!==promises.pipeline||a.finished!==promises.finished||require('stream').promises!==require('node:stream/promises')||require('stream/promises')!==require('rustyjs:stream/promises'))throw Error('promise aliases');const original=a.pipeline;promises.pipeline=()=>{};if((await import('stream/promises')).pipeline!==original||require('stream').pipeline!==pipeline||require('stream').finished!==finished)throw Error('captured binding');console.log('stream helper aliases');",
        b"stream helper aliases\n",
    );
}

#[test]
fn for_await_consumes_sync_async_and_falsy_objects_and_waits_for_close() {
    succeeds(
        "let finalized=0;const source=Readable.from((async function*(){try{yield undefined;yield 0;await Rusty.sleep(0);yield false;}finally{finalized++;}})());const values=[];for await(const value of source)values.push(value);if(values.length!==3||values[0]!==undefined||values[1]!==0||values[2]!==false||finalized!==1||!source.closed||source.listenerCount('error')!==0)throw Error('iteration completion');const bytes=[];for await(const chunk of Readable.from(['a','b','c'],{objectMode:false}))bytes.push(chunk);if(Buffer.concat(bytes).toString()!=='abc')throw Error('byte iteration');console.log('stream async iteration');",
        b"stream async iteration\n",
    );
}

#[test]
fn concurrent_next_calls_are_fifo_and_iterator_creation_is_lazy() {
    succeeds(
        "let pulls=0,returns=0;const source=Readable.from({[Symbol.iterator](){return this;},next(){pulls++;return {done:pulls>3,value:pulls};},return(){returns++;return {done:true};}},{highWaterMark:1});const iterator=source[Symbol.asyncIterator]();if(pulls!==0||iterator[Symbol.asyncIterator]()!==iterator)throw Error('lazy iterator');const values=await Promise.all([iterator.next(),iterator.next(),iterator.next()]);if(values.map(x=>x.value).join(',')!=='1,2,3'||values.some(x=>x.done))throw Error('FIFO');if(!(await iterator.next()).done||!source.closed||returns!==0||source.listenerCount('error'))throw Error('natural completion');console.log('stream concurrent next');",
        b"stream concurrent next\n",
    );
}

#[test]
fn early_return_cancels_pending_next_and_awaits_async_cleanup_without_close_event() {
    succeeds(
        "let timer,cleanups=0,cleaned=false;const source=new Readable({emitClose:false,read(){timer=setTimeout(()=>this.push('late'),10000);},destroy(reason,done){cleanups++;clearTimeout(timer);setTimeout(()=>{cleaned=true;done(reason);},0);}});const iterator=source[Symbol.asyncIterator]();const pending=iterator.next();await Rusty.sleep(0);const result=await iterator.return(42);if(!result.done||result.value!==42||!(await pending).done||!cleaned||cleanups!==1||!source.closed||source.listenerCount('error'))throw Error('return cleanup');console.log('stream iterator cancellation');",
        b"stream iterator cancellation\n",
    );
}

#[test]
fn break_destroys_by_default_and_preserving_return_allows_later_consumption() {
    succeeds(
        "let cleaned=0;async function* generate(){try{yield 1;yield 2;}finally{cleaned++;}}const source=Readable.from(generate(),{highWaterMark:1});for await(const value of source){if(value!==1)throw Error('first value');break;}if(!source.closed||cleaned!==1||source.listenerCount('error'))throw Error('break cleanup');const preserved=Readable.from([1,2,3],{highWaterMark:1});for await(const value of preserved.iterator({destroyOnReturn:false})){if(value!==1)throw Error('preserved first');break;}if(preserved.destroyed||preserved.listenerCount('error'))throw Error('preserve mode');const rest=[];for await(const value of preserved)rest.push(value);if(rest.join(',')!=='2,3'||!preserved.closed)throw Error('resume iteration');console.log('stream iterator return modes');",
        b"stream iterator return modes\n",
    );
}

#[test]
fn iterator_throw_and_producer_failure_reject_with_original_values_after_cleanup() {
    succeeds(
        "const sentinel=new Error('iterator abort');const source=new Readable({read(){}});const iterator=source[Symbol.asyncIterator]();const pending=iterator.next();const aborted=iterator.throw(sentinel);const results=await Promise.allSettled([pending,aborted]);if(results.some(x=>x.status!=='rejected'||x.reason!==sentinel)||!source.closed||source.listenerCount('error'))throw Error('throw cleanup');const producer=Readable.from({[Symbol.iterator](){return this;},next(){throw sentinel;},return(){return {done:true};}});try{await producer[Symbol.asyncIterator]().next();throw Error('failure ignored');}catch(e){if(e!==sentinel||!producer.closed||producer.listenerCount('error'))throw e;}console.log('stream iterator errors');",
        b"stream iterator errors\n",
    );
}

#[test]
fn premature_close_and_invalid_iterator_options_are_controlled() {
    succeeds(
        "const source=new Readable({read(){}});for(const options of [null,{destroyOnReturn:1}]){try{source.iterator(options);throw Error('bad options');}catch(e){if(!(e instanceof TypeError))throw e;}}const iterator=source[Symbol.asyncIterator]();const next=iterator.next();source.destroy();try{await next;throw Error('premature close ignored');}catch(e){if(e.code!=='ERR_STREAM_PREMATURE_CLOSE')throw e;}if(source.listenerCount('error'))throw Error('guard retained');console.log('stream iterator validation');",
        b"stream iterator validation\n",
    );
}

#[test]
fn finished_callback_and_promise_wait_for_cleanup_and_remove_opted_in_guards() {
    succeeds(
        "let cleaned=false,callbacks=0;const sink=new Writable({emitClose:false,write(c,e,d){d();},destroy(reason,done){setTimeout(()=>{cleaned=true;done(reason);},0);}});const callbackDone=new Promise((resolve,reject)=>{finished(sink,{cleanup:true},reason=>{callbacks++;if(reason)reject(reason);else resolve();});});const promised=promises.finished(sink,{cleanup:true});sink.end('x');await Promise.all([callbackDone,promised]);if(callbacks!==1||!cleaned||!sink.closed||sink.listenerCount('error'))throw Error('finished cleanup');const source=Readable.from([1]);const done=promises.finished(source,{cleanup:true});source.resume();await done;if(!source.closed||source.listenerCount('error'))throw Error('readable finished');console.log('stream finished cleanup');",
        b"stream finished cleanup\n",
    );
}

#[test]
fn finished_defaults_retain_error_guard_until_explicit_cleanup_and_cancel_is_idempotent() {
    succeeds(
        "const sink=new Writable({autoDestroy:false,write(c,e,d){d();}});let calls=0;const done=new Promise(resolve=>{const cleanup=finished(sink,()=>{calls++;if(sink.listenerCount('error')!==1)throw Error('default guard');cleanup();cleanup();resolve();});});sink.end();await done;if(calls!==1||sink.listenerCount('error')||sink.destroyed)throw Error('explicit cleanup');const canceled=new Writable({write(c,e,d){d();}});const cleanup=finished(canceled,()=>{throw Error('canceled callback');});cleanup();cleanup();canceled.end();await promises.finished(canceled,{cleanup:true});console.log('stream finished guards');",
        b"stream finished guards\n",
    );
}

#[test]
fn finished_detects_errors_premature_close_and_already_completed_streams() {
    succeeds(
        "const sentinel=new Error('write failed');const sink=new Writable({write(c,e,d){d(sentinel);}});const failed=promises.finished(sink,{cleanup:true});sink.end('x');try{await failed;throw Error('failure ignored');}catch(e){if(e!==sentinel||!sink.closed)throw e;}const premature=new Readable({read(){}});const close=promises.finished(premature,{cleanup:true});premature.destroy();try{await close;throw Error('premature ignored');}catch(e){if(e.code!=='ERR_STREAM_PREMATURE_CLOSE')throw e;}await promises.finished(sink,{cleanup:true}).then(()=>{throw Error('cached error lost');},e=>{if(e!==sentinel)throw e;});const complete=new Writable({autoDestroy:false,write(c,e,d){d();}});complete.end();await promises.finished(complete,{cleanup:true});await promises.finished(complete,{cleanup:true});if(complete.listenerCount('error'))throw Error('completed guards');console.log('stream finished outcomes');",
        b"stream finished outcomes\n",
    );
}

#[test]
fn finished_direction_and_error_options_are_explicit() {
    succeeds(
        "const idle=new Readable({read(){}});await promises.finished(idle,{readable:false,cleanup:true});if(idle.destroyed||idle.readableDidRead)throw Error('finished consumed');const sink=new Writable({write(c,e,d){d(new Error('ignored by observer'));}});sink.on('error',()=>{});const done=promises.finished(sink,{error:false,cleanup:true});sink.end('x');try{await done;throw Error('premature ignored');}catch(e){if(e.code!=='ERR_STREAM_PREMATURE_CLOSE')throw e;}idle.destroy();console.log('stream finished options');",
        b"stream finished options\n",
    );
}

#[test]
fn pipeline_success_supervises_async_endpoints_and_cleans_every_guard() {
    succeeds(
        "const source=Readable.from(['a','b'],{objectMode:false,highWaterMark:1});const chunks=[];const sink=new Writable({emitClose:false,highWaterMark:1,write(c,e,d){setTimeout(()=>{chunks.push(c.toString());d();},0);}});const done=new Promise((resolve,reject)=>{if(pipeline(source,sink,reason=>reason?reject(reason):resolve())!==sink)throw Error('pipeline return');});await done;if(chunks.join(',')!=='a,b'||!source.closed||!sink.closed||source.listenerCount('error')||source.listenerCount('data')||sink.listenerCount('error')||sink.listenerCount('drain'))throw Error('pipeline cleanup');console.log('stream pipeline success');",
        b"stream pipeline success\n",
    );
}

#[test]
fn promise_pipeline_accepts_iterables_array_form_and_end_false_flushes_without_ending_sink() {
    succeeds(
        "const chunks=[];const sink=new Writable({objectMode:true,highWaterMark:1,write(c,e,d){setTimeout(()=>{chunks.push(c);d();},0);}});await promises.pipeline([[1,2,3],sink],{end:false});if(chunks.join(',')!=='1,2,3'||sink.writableEnded||sink.writableLength||sink.listenerCount('error'))throw Error('end false');await promises.pipeline([4,5],sink);if(chunks.join(',')!=='1,2,3,4,5'||!sink.closed)throw Error('iterable reuse');console.log('stream pipeline forms');",
        b"stream pipeline forms\n",
    );
}

#[test]
fn pipeline_failure_cancels_peer_waits_for_cleanup_and_preserves_first_error() {
    succeeds(
        "const first=new Error('first failure'),later=new Error('cleanup failure');let returns=0,cleaned=false;const source=Readable.from({[Symbol.iterator](){return this;},next(){return {done:false,value:'x'};},return(){returns++;return {done:true};}},{highWaterMark:1});const sink=new Writable({objectMode:true,highWaterMark:1,write(c,e,d){d(first);},destroy(reason,done){setTimeout(()=>{cleaned=true;done(later);},0);}});try{await promises.pipeline(source,sink);throw Error('pipeline failure lost');}catch(e){if(e!==first||!cleaned||returns!==1||!source.closed||!sink.closed)throw e;}if(source.listenerCount('error')||source.listenerCount('data')||sink.listenerCount('error')||sink.listenerCount('drain'))throw Error('failed pipeline guards');console.log('stream pipeline failure');",
        b"stream pipeline failure\n",
    );
}

#[test]
fn pipeline_source_errors_and_error_free_premature_close_destroy_destination() {
    succeeds(
        "for(const withError of [false,true]){const failure=new Error('source failed');const source=new Readable({read(){this.destroy(withError?failure:undefined);}});let cleaned=0;const sink=new Writable({write(c,e,d){d();},destroy(reason,done){cleaned++;done(reason);}});try{await promises.pipeline(source,sink);throw Error('source failure ignored');}catch(e){if(withError?e!==failure:e.code!=='ERR_STREAM_PREMATURE_CLOSE')throw e;}if(cleaned!==1||!sink.closed||!source.closed||sink.listenerCount('error'))throw Error('peer cleanup');}console.log('stream pipeline source failures');",
        b"stream pipeline source failures\n",
    );
}

#[test]
fn pipeline_and_finished_validate_before_acquiring_iterators_or_starting_io() {
    succeeds(
        "let acquired=0;const iterable={[Symbol.iterator](){acquired++;return [][Symbol.iterator]();}};const sink=new Writable({write(c,e,d){d();}});for(const action of [()=>pipeline(iterable,{},()=>{}),()=>pipeline(iterable,sink,{end:1},()=>{}),()=>pipeline(iterable,sink),()=>finished(sink,{cleanup:1},()=>{}),()=>finished({},()=>{}),()=>finished(sink,{signal:{}},()=>{})]){try{action();throw Error('validation accepted');}catch(e){if(!(e instanceof TypeError))throw e;}}if(acquired||sink.listenerCount('error'))throw Error('validation side effects');await promises.pipeline(iterable,{}).then(()=>{throw Error('promise validation accepted');},e=>{if(!(e instanceof TypeError))throw e;});sink.destroy();console.log('stream helper validation');",
        b"stream helper validation\n",
    );
}

#[test]
fn simultaneous_endpoint_failures_keep_the_first_recorded_error() {
    succeeds(
        "for(const destinationFirst of [false,true]){const a=new Error('source'),b=new Error('destination');const source=new Readable({read(){}}),sink=new Writable({write(c,e,d){d();}});const done=promises.pipeline(source,sink);if(destinationFirst){sink.destroy(b);source.destroy(a);}else{source.destroy(a);sink.destroy(b);}try{await done;throw Error('failure lost');}catch(e){if(e!==(destinationFirst?b:a))throw e;}if(!source.closed||!sink.closed||source.listenerCount('error')||sink.listenerCount('error'))throw Error('simultaneous cleanup');}console.log('stream first failure');",
        b"stream first failure\n",
    );
}

#[test]
fn helper_observation_setup_rolls_back_on_metadata_failure() {
    succeeds(
        "const sentinel=new Error('observer setup');const source=new Readable({read(){}}),sink=new Writable({write(c,e,d){d();}});sink.on('newListener',name=>{if(name==='error')throw sentinel;});try{pipeline(source,sink,()=>{throw Error('setup callback fired');});throw Error('setup ignored');}catch(e){if(e!==sentinel)throw e;}if(source.listenerCount('error')||sink.listenerCount('error')||source.listenerCount('data')||source.destroyed||sink.destroyed)throw Error('setup side effects');try{finished(sink,()=>{});throw Error('finished setup ignored');}catch(e){if(e!==sentinel)throw e;}if(sink.listenerCount('error'))throw Error('finished guard retained');source.destroy();sink.destroy();console.log('stream observer rollback');",
        b"stream observer rollback\n",
    );
}

#[test]
fn cleanup_continues_when_pipeline_callback_or_observer_removal_throws() {
    let output = run(
        "const source=Readable.from(['x']);const sink=new Writable({objectMode:true,write(c,e,d){d();}});source.on('removeListener',name=>{if(name==='error')throw Error('observer removal threw');});pipeline(source,sink,()=>{if(!source.closed||!sink.closed||sink.listenerCount('error')||source.listenerCount('error'))throw Error('cleanup incomplete');console.log('pipeline cleaned');throw Error('pipeline callback threw');});",
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("observer removal threw"));
    assert_eq!(output.stdout, b"pipeline cleaned\n");
}

#[test]
fn terminated_iterators_are_done_and_manual_failure_waiters_settle_once() {
    succeeds(
        "const reason=new Error('stopped');const source=new Readable({read(){}});const iterator=source[Symbol.asyncIterator]();const pending=iterator.next();const thrown=iterator.throw(reason);await Promise.allSettled([pending,thrown]);if(!(await iterator.next()).done||(await iterator.return(7)).value!==7||source.listenerCount('error'))throw Error('terminal protocol');let callbacks=0;const sink=new Writable({write(c,e,d){d();}});const done=new Promise(resolve=>{finished(sink,{cleanup:true},e=>{callbacks++;if(e!==reason)throw Error('manual reason');resolve();});});sink.emit('error',reason);await done;sink.end();await promises.finished(sink,{cleanup:true});if(callbacks!==1)throw Error('callback repeated');console.log('stream terminal protocol');",
        b"stream terminal protocol\n",
    );
}

#[test]
fn pipeline_respects_manual_lifecycle_and_waits_when_end_false_destination_finishes_itself() {
    succeeds(
        "const source=Readable.from([1],{autoDestroy:false});const sink=new Writable({autoDestroy:false,objectMode:true,write(c,e,d){d();}});await promises.pipeline(source,sink);if(!source.readableEnded||!sink.writableFinished||source.destroyed||sink.destroyed)throw Error('manual lifecycle');source.destroy();sink.destroy();let cleaned=false;const selfEnding=new Writable({objectMode:true,write(c,e,d){d();},final(done){setTimeout(done,0);},destroy(reason,done){setTimeout(()=>{cleaned=true;done(reason);},0);}});const selfSource=Readable.from([1]);selfSource.on('end',()=>selfEnding.end());await promises.pipeline(selfSource,selfEnding,{end:false});if(!cleaned||!selfEnding.closed||selfEnding.listenerCount('error'))throw Error('end false cleanup race');console.log('stream pipeline lifecycle modes');",
        b"stream pipeline lifecycle modes\n",
    );
}

#[test]
fn iterator_return_reports_cleanup_failure_and_setup_exceptions_leave_no_guard() {
    succeeds(
        "const sentinel=new Error('iterator cleanup failed');const source=Readable.from({[Symbol.iterator](){return this;},next(){return {done:false,value:1};},return(){return Promise.reject(sentinel);}});const iterator=source[Symbol.asyncIterator]();try{await iterator.return();throw Error('cleanup failure lost');}catch(e){if(e!==sentinel||!source.closed||source.listenerCount('error'))throw e;}const untouched=new Readable({read(){throw Error('unexpected pull');}});untouched.on('newListener',name=>{if(name==='error')throw sentinel;});try{await untouched[Symbol.asyncIterator]().next();throw Error('setup failure lost');}catch(e){if(e!==sentinel||untouched.listenerCount('error')||untouched.destroyed)throw e;}untouched.destroy();console.log('stream iterator setup and cleanup failures');",
        b"stream iterator setup and cleanup failures\n",
    );
}
