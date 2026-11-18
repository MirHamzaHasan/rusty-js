mod common;

fn run(body: &str) -> std::process::Output {
    common::run(&[
        "-e",
        &format!("import('node:events').then(async ({{default: EventEmitter}}) => {{ {body} }});"),
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
fn builtin_aliases_and_commonjs_share_the_constructor() {
    succeeds(
        "const [a,b,c] = await Promise.all([import('node:events'),import('events'),import('rustyjs:events')]); if (a !== b || b !== c || a.EventEmitter !== EventEmitter) throw Error('ESM aliases'); const {createRequire} = await import('rustyjs:module'); const require = createRequire('/virtual.cjs'); const Ctor = require('events'); if (typeof Ctor !== 'function' || Ctor !== EventEmitter || Ctor.EventEmitter !== Ctor || require('node:events') !== Ctor || require('rustyjs:events') !== Ctor || require.resolve('events') !== 'events' || require.resolve('node:events') !== 'node:events') throw Error('CJS aliases'); console.log('event aliases');",
        b"event aliases\n",
    );
}

#[test]
fn synchronous_order_arguments_receiver_duplicates_and_legacy_construction() {
    succeeds(
        "function Legacy(){EventEmitter.call(this);} Legacy.prototype = Object.create(EventEmitter.prototype); const e = new Legacy(); const value = {}; const calls=[]; function a(x,y){if(this!==e || x!==value || y!==42) throw Error('arguments'); calls.push('a');} function b(){calls.push('b');} if(e.on('x',a)!==e || e.addListener!==e.on || e.off!==e.removeListener) throw Error('chaining/aliases'); e.on('x',a).prependListener('x',b); if(e.listenerCount('x',a)!==2 || !e.emit('x',value,42) || e.emit('missing')!==false || calls.join(',')!=='b,a,a') throw Error('order'); class Child extends EventEmitter {} const child=new Child(); child.on('ready',()=>calls.push('child')); child.emit('ready'); console.log(calls.join(','));",
        b"b,a,a,child\n",
    );
}

#[test]
fn mutation_during_emit_uses_snapshots_and_removal_is_most_recent() {
    succeeds(
        "const e=new EventEmitter(); const calls=[]; function second(){calls.push('second');} function third(){calls.push('third');} e.on('x',()=>{calls.push('first'); e.off('x',second); e.on('x',third);}).on('x',second); e.emit('x'); e.emit('x'); if(calls.join(',')!=='first,second,first,third') throw Error('snapshot'); const copy=e.listeners('x'); copy.length=0; if(e.listenerCount('x')!==3) throw Error('copy leaked'); const duplicate=()=>{}; e.on('dup',duplicate).once('dup',duplicate); e.removeListener('dup',duplicate); if(e.rawListeners('dup')[0]!==duplicate || e.listenerCount('dup')!==1) throw Error('LIFO removal'); console.log('mutation snapshots');",
        b"mutation snapshots\n",
    );
}

#[test]
fn once_wrappers_remove_before_reentry_and_keep_original_introspection() {
    succeeds(
        "const e=new EventEmitter(); let count=0; function once(){if(this!==e || e.listenerCount('x')!==0) throw Error('once state'); count++; e.emit('x');} e.once('x',once); const raw=e.rawListeners('x')[0]; if(raw===once || raw.listener!==once || e.listeners('x')[0]!==once) throw Error('once introspection'); raw.call(null); raw(); e.emit('x'); if(count!==1) throw Error('once fired twice'); const order=[]; e.on('p',()=>order.push('ordinary')); e.prependOnceListener('p',()=>order.push('once')); e.emit('p'); e.emit('p'); if(order.join(',')!=='once,ordinary,ordinary') throw Error('prepend once'); console.log('once reentry');",
        b"once reentry\n",
    );
}

#[test]
fn listener_meta_events_observe_before_add_and_after_removal() {
    succeeds(
        "const e=new EventEmitter(); const calls=[]; function a(){calls.push('a');} function b(){calls.push('b');} e.once('newListener',(name,listener)=>{if(name!=='x'||listener!==a||e.listenerCount('x')!==0)throw Error('newListener order'); e.on('x',b);}); e.on('x',a); e.emit('x'); if(calls.join(',')!=='b,a')throw Error('meta insertion'); const removed=[]; e.on('removeListener',(name,listener)=>{if(name==='x')removed.push(listener.name);}); e.removeAllListeners('x'); if(removed.join(',')!=='a,b'||e.listenerCount('x')!==0)throw Error('remove meta order'); e.once('y',a); e.removeAllListeners(); if(e.eventNames().length!==0)throw Error('remove all'); console.log('listener metadata');",
        b"listener metadata\n",
    );
}

#[test]
fn event_keys_support_symbols_numeric_names_and_prototype_like_strings() {
    succeeds(
        "const e=new EventEmitter(); const symbol=Symbol('key'); const calls=[]; let coercions=0; const objectKey={[Symbol.toPrimitive](){coercions++;return symbol;}}; e.on('__proto__',()=>calls.push('proto')).on(2,()=>calls.push('two')).on('1',()=>calls.push('one')).on(objectKey,()=>calls.push('symbol')); if(coercions!==1)throw Error('key conversion'); const names=e.eventNames(); if(names[0]!=='1'||names[1]!=='2'||names[2]!=='__proto__'||names[3]!==symbol)throw Error('property key order'); e.emit('__proto__');e.emit(2);e.emit('1');e.emit(symbol);if(calls.join(',')!=='proto,two,one,symbol')throw Error('keys'); console.log('event keys');",
        b"event keys\n",
    );
}

#[test]
fn errors_propagate_original_values_and_stop_dispatch() {
    succeeds(
        "const e=new EventEmitter(); const error=new Error('original'); try{e.emit('error',error);throw Error('error event ignored');}catch(caught){if(caught!==error)throw caught;} try{e.emit('error',42);throw Error('nonerror ignored');}catch(caught){if(!(caught instanceof Error)||caught.context!==42||!caught.message.includes('Unhandled'))throw caught;} e.on('error',value=>{if(value!==error)throw Error('error listener');});if(!e.emit('error',error))throw Error('handled error');const sentinel={};let later=false;e.on('x',()=>{throw sentinel;}).on('x',()=>later=true);try{e.emit('x');throw Error('listener throw lost');}catch(caught){if(caught!==sentinel||later)throw Error('dispatch did not stop');}console.log('event errors');",
        b"event errors\n",
    );
}

#[test]
fn promise_returns_are_ignored_and_async_work_uses_the_runtime_loop() {
    succeeds(
        "const e=new EventEmitter();const order=[];e.on('x',async()=>{order.push('sync');await Rusty.sleep(0);order.push('async');});order.push('before');if(e.emit('x')!==true)throw Error('emit returned promise');order.push('after');if(order.join(',')!=='before,sync,after')throw Error('synchronous emit');await Rusty.sleep(2);if(order.join(',')!=='before,sync,after,async')throw Error('async drain');console.log('event async work');",
        b"event async work\n",
    );
    let output = run(
        "const e=new EventEmitter();e.on('x',async()=>{throw Error('listener rejection');});e.emit('x');",
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("listener rejection"));
}

#[test]
fn max_listener_warnings_do_not_drop_listeners_and_limits_are_configurable() {
    let output = run(
        "const e=new EventEmitter();e.setMaxListeners(1);const a=()=>{};e.on('x',a).on('x',a).on('x',a);if(e.listenerCount('x')!==3)throw Error('warning dropped listener');e.removeListener('x',a).removeListener('x',a);e.on('x',a);const unlimited=new EventEmitter().setMaxListeners(0);for(let i=0;i<20;i++)unlimited.on('x',a);EventEmitter.defaultMaxListeners=Infinity;const other=new EventEmitter();if(other.getMaxListeners()!==Infinity||e.getMaxListeners()!==1)throw Error('limits');for(const value of [-1,NaN]){try{e.setMaxListeners(value);throw Error('bad limit');}catch(error){if(!(error instanceof RangeError))throw error;}}console.log('listener limits');",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"listener limits\n");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr)
            .matches("MaxListenersExceededWarning")
            .count(),
        2
    );
}

#[test]
fn invalid_listeners_capture_options_and_receivers_fail_explicitly() {
    succeeds(
        "const e=new EventEmitter();for(const method of ['on','once','prependListener','prependOnceListener','removeListener']){try{e[method]('x',{});throw Error('bad listener accepted');}catch(error){if(!(error instanceof TypeError))throw error;}}for(const action of [()=>new EventEmitter({captureRejections:true}),()=>{EventEmitter.captureRejections=true;},()=>e.on.call({},'x',()=>{}),()=>e.setMaxListeners('1')]){try{action();throw Error('unsupported behavior accepted');}catch(error){if(!(error instanceof TypeError))throw error;}}console.log('event validation');",
        b"event validation\n",
    );
}

#[test]
fn captured_intrinsics_survive_method_replacement_after_bootstrap() {
    succeeds(
        "const OriginalError=Error;WeakMap.prototype.get=()=>{throw new OriginalError('patched weak get');};WeakMap.prototype.set=()=>{throw new OriginalError('patched weak set');};Array.prototype.push=()=>{throw new OriginalError('patched push');};Array.prototype.slice=()=>{throw new OriginalError('patched slice');};Reflect.apply=()=>{throw new OriginalError('patched apply');};const e=new EventEmitter();let calls=0;e.once('x',()=>calls++);e.emit('x');if(calls!==1||e.listeners('x').length!==0)throw new OriginalError('captured intrinsics');console.log('event intrinsics');",
        b"event intrinsics\n",
    );
}
