'use strict';

const uncurry = fn => Function.prototype.call.bind(fn);
const apply = Reflect.apply, define = Object.defineProperty;
const ownKeys = Reflect.ownKeys;
const ErrorCtor = Error, TypeErrorCtor = TypeError, RangeErrorCtor = RangeError;
const integer = Number.isInteger, min = Math.min, max = Math.max;
const weakGet = uncurry(WeakMap.prototype.get), weakSet = uncurry(WeakMap.prototype.set);
const pushArray = uncurry(Array.prototype.push), shift = uncurry(Array.prototype.shift);
const unshiftArray = uncurry(Array.prototype.unshift), splice = uncurry(Array.prototype.splice);
const indexOf = uncurry(Array.prototype.indexOf);
const resolvePromise = Promise.resolve.bind(Promise);
const PromiseCtor = Promise;
const then = uncurry(Promise.prototype.then), checkpoint = Promise.resolve();
const defer = fn => { then(checkpoint, () => { fn(); }); };
const emit = uncurry(EventEmitter.prototype.emit);
const on = uncurry(EventEmitter.prototype.on), once = uncurry(EventEmitter.prototype.once);
const prepend = uncurry(EventEmitter.prototype.prependListener);
const prependOnce = uncurry(EventEmitter.prototype.prependOnceListener);
const remove = uncurry(EventEmitter.prototype.removeListener);
const countListeners = uncurry(EventEmitter.prototype.listenerCount);
const bufferFrom = Buffer.from, alloc = Buffer.alloc, isBuffer = Buffer.isBuffer;
const isEncoding = Buffer.isEncoding, copyBytes = uncurry(Buffer.prototype.copy);
const subarray = uncurry(Buffer.prototype.subarray);
const typedPrototype = Object.getPrototypeOf(Uint8Array.prototype);
const byteLength = uncurry(Object.getOwnPropertyDescriptor(typedPrototype, 'byteLength').get);
const tag = uncurry(Object.getOwnPropertyDescriptor(typedPrototype, Symbol.toStringTag).get);
const validateBytes = uncurry(Uint8Array.prototype.values);
const iteratorSymbol = Symbol.iterator, asyncIteratorSymbol = Symbol.asyncIterator;
const states = new WeakMap();

function stateOf(stream) {
    const state = weakGet(states, stream);
    if (!state) throw new TypeErrorCtor('incompatible Readable receiver');
    return state;
}
function error(message, code) { const result = new ErrorCtor(message); result.code = code; return result; }
function bool(options, name, fallback) {
    const value = options[name];
    if (value === undefined) return fallback;
    if (typeof value !== 'boolean') throw new TypeErrorCtor(`${name} must be a boolean`);
    return value;
}
function blocked(state) {
    for (let i = 0; i < state.pipes.length; i++) if (state.pipes[i].blocked) return true;
    return false;
}
function schedule(stream, state) {
    if (state.scheduled || state.destroyed) return;
    state.scheduled = true;
    defer(() => { state.scheduled = false; drive(stream, state); });
}
function notify(stream, state) {
    Lifecycle.notify(stream);
    if (state.notifying || state.destroyed) return;
    state.notifying = true;
    defer(() => {
        state.notifying = false;
        if (!state.destroyed && (state.length > 0 || state.eof)) emit(stream, 'readable');
    });
}
function setFlow(stream, state, flowing) {
    if (state.flowing === flowing) return;
    state.flowing = flowing;
    // Internal notifications are jobs so a listener throw cannot interrupt cleanup.
    defer(() => { if (!state.destroyed) emit(stream, flowing ? 'resume' : 'pause'); });
}
function resumePipes(stream, state) {
    if (state.destroyed) return;
    if (!state.userPaused && !blocked(state) && countListeners(stream, 'readable') === 0) {
        setFlow(stream, state, true);
        schedule(stream, state);
    }
}
function detach(stream, state, record) {
    // newListener/removeListener are guest callbacks. Complete every removal even
    // if one throws, and repeat binding cleanup after a reentrant setup abort.
    const alreadyRemoved = record.removed;
    record.removed = true;
    // Remove registry identity before any guest metadata callback can re-enter
    // destroy/unpipe. Otherwise a nested destroy could loop over this same record.
    const index = indexOf(state.pipes, record);
    if (index >= 0) splice(state.pipes, index, 1);
    record.blocked = false;
    if (!alreadyRemoved) {
        if (state.pipes.length === 0) { state.userPaused = true; setFlow(stream, state, false); }
        else resumePipes(stream, state);
    }
    const clean = (target, event, listener) => {
        try { remove(target, event, listener); }
        catch (caught) { defer(() => { throw caught; }); }
    };
    clean(stream, 'data', record.data);
    clean(stream, 'end', record.end);
    clean(record.destination, 'drain', record.drain);
    clean(record.destination, 'error', record.error);
    clean(record.destination, 'finish', record.close);
    clean(record.destination, 'close', record.close);
    if (alreadyRemoved) return;
    defer(() => emit(record.destination, 'unpipe', stream, {hasUnpiped: true}));
}
function destroy(stream, state, reason) {
    if (state.destroyed) return stream;
    state.destroyed = true;
    if (reason != null) state.error = reason;
    state.queue = [];
    state.length = 0;
    state.reading = false;
    state.flowing = false;
    Lifecycle.notify(stream);
    while (state.pipes.length) detach(stream, state, state.pipes[0]);
    let called = false;
    const done = result => {
        if (called) return;
        called = true;
        defer(() => {
            if (result != null) state.error = result;
            if (state.error != null) defer(() => emit(stream, 'error', state.error));
            defer(() => { state.closed = true; Lifecycle.notify(stream); if (state.emitClose) emit(stream, 'close'); });
        });
    };
    try { apply(stream._destroy, stream, [state.error, done]); }
    catch (caught) { if (called) state.error = caught; else done(caught); }
    return stream;
}
function maybeEnd(stream, state) {
    if (!state.eof || state.length !== 0 || !state.demand || state.endScheduled || state.ended || state.destroyed) return;
    state.endScheduled = true;
    defer(() => {
        state.endScheduled = false;
        if (state.destroyed || state.ended || state.length !== 0) return;
        state.ended = true;
        Lifecycle.notify(stream);
        try { emit(stream, 'end'); }
        finally { if (state.autoDestroy) defer(() => destroy(stream, state)); }
    });
}
function pull(stream, state) {
    if (state.destroyed || state.eof || state.reading || state.inRead || !state.demand) return;
    if (state.length > 0 && state.length >= state.highWaterMark) return;
    state.reading = true;
    state.inRead = true;
    try { apply(stream._read, stream, [state.highWaterMark]); }
    catch (caught) { destroy(stream, state, caught); }
    finally { state.inRead = false; }
}
function consume(state, size) {
    if (state.objectMode) { state.length--; return shift(state.queue).chunk; }
    // Validate all involved storage before changing the queue. Caller-owned Buffers
    // may have been detached after push; no partial dequeue should survive failure.
    let checked = 0;
    for (let i = 0; checked < size; i++) { validateBytes(state.queue[i].chunk); checked += state.queue[i].size; }
    const first = state.queue[0];
    if (size <= first.size) {
        const result = size === first.size ? shift(state.queue).chunk : subarray(first.chunk, 0, size);
        if (size < first.size) { first.chunk = subarray(first.chunk, size); first.size -= size; }
        state.length -= size;
        return result;
    }
    const result = alloc(size);
    let offset = 0;
    while (offset < size) {
        const record = state.queue[0], count = min(record.size, size - offset);
        copyBytes(record.chunk, result, offset, 0, count);
        offset += count;
        if (count === record.size) shift(state.queue);
        else { record.chunk = subarray(record.chunk, count); record.size -= count; }
    }
    state.length -= size;
    return result;
}
function take(stream, state, size) {
    let chunk;
    try { chunk = consume(state, size); }
    catch (caught) { destroy(stream, state, caught); return null; }
    state.didRead = true;
    Lifecycle.notify(stream);
    schedule(stream, state);
    emit(stream, 'data', chunk);
    return chunk;
}
function drive(stream, state) {
    if (state.destroyed) return;
    pull(stream, state);
    if (state.destroyed) return;
    if (state.flowing && !blocked(state) && state.queue.length && countListeners(stream, 'readable') === 0) {
        take(stream, state, state.objectMode ? 1 : state.queue[0].size);
    }
    maybeEnd(stream, state);
}
function normalize(state, chunk, encoding) {
    if (state.objectMode) return {chunk, size: 1};
    if (chunk === undefined) return {chunk: null, size: 0};
    if (typeof chunk === 'string') {
        if (encoding !== undefined && !isEncoding(encoding)) throw new TypeErrorCtor('unsupported stream encoding');
        chunk = bufferFrom(chunk, encoding);
    } else {
        if (tag(chunk) !== 'Uint8Array') throw new TypeErrorCtor('byte chunks must be strings, Buffer, or Uint8Array');
        validateBytes(chunk);
        if (!isBuffer(chunk)) chunk = bufferFrom(chunk);
    }
    return {chunk, size: byteLength(chunk)};
}
function subscribe(stream, method, name, listener) {
    const state = stateOf(stream);
    name = ownKeys({[name]: true})[0];
    method(stream, name, listener);
    if (name === 'data') {
        state.demand = true;
        if (state.flowing === null && countListeners(stream, 'readable') === 0) {
            state.userPaused = false;
            resumePipes(stream, state);
        }
    } else if (name === 'readable') {
        state.demand = true;
        state.flowing = false;
        if (state.length || state.eof) notify(stream, state);
    }
    if (name === 'data' || name === 'readable') schedule(stream, state);
    return stream;
}

function createIterator(stream, options) {
    const state = stateOf(stream);
    if (options === null || typeof options !== 'object') throw new TypeErrorCtor('iterator options must be an object');
    const destroyOnReturn = bool(options, 'destroyOnReturn', true);
    const pending = [], endings = [];
    let started = false, terminal = false, closing = false, scheduled = false;
    let failed = false, failure, unwatch;
    const fail = reason => { if (!failed) { failed = true; failure = reason; } };
    const request = () => {
        if (scheduled || terminal) return;
        scheduled = true;
        defer(() => { scheduled = false; pumpIterator(); });
    };
    const onError = reason => {
        fail(reason);
        if (!state.destroyed) destroy(stream, state, reason);
        request();
    };
    const cleanup = () => {
        if (unwatch) { unwatch(); unwatch = undefined; }
        try { remove(stream, 'error', onError); }
        catch (caught) { defer(() => { throw caught; }); }
    };
    const complete = () => {
        terminal = true;
        cleanup();
        while (pending.length) {
            const waiter = shift(pending);
            if (failed) waiter.reject(failure); else waiter.resolve({value: undefined, done: true});
        }
        while (endings.length) {
            const waiter = shift(endings);
            if (failed) waiter.reject(failure); else waiter.resolve({value: waiter.value, done: true});
        }
    };
    const start = () => {
        if (started) return;
        started = true;
        unwatch = Lifecycle.watch(stream, request);
        try { prepend(stream, 'error', onError); }
        catch (caught) { fail(caught); complete(); }
    };
    function pumpIterator() {
        if (terminal) return;
        const recorded = Lifecycle.inspect(stream).failure;
        if (recorded) fail(recorded.reason);
        if (failed && !state.destroyed) destroy(stream, state, failure);
        if (state.destroyed) {
            if (!state.closed) return;
            if (!closing && !state.ended && !failed) fail(error('Stream closed before end', 'ERR_STREAM_PREMATURE_CLOSE'));
            complete();
            return;
        }
        if (closing) { complete(); return; }
        if (state.ended) {
            if (!state.autoDestroy) complete(); // Otherwise wait for cleanup, even with emitClose=false.
            return;
        }
        while (pending.length && !state.destroyed) {
            let chunk;
            try { chunk = readForIterator(stream); }
            catch (caught) { fail(caught); destroy(stream, state, caught); return; }
            if (chunk === null) return;
            shift(pending).resolve({value: chunk, done: false});
        }
    }
    const iterator = {
        next() {
            return new PromiseCtor((resolve, reject) => {
                if (terminal) { resolve({value: undefined, done: true}); return; }
                pushArray(pending, {resolve, reject});
                start(); request();
            });
        },
        return(value) {
            return new PromiseCtor((resolve, reject) => {
                if (terminal) { resolve({value, done: true}); return; }
                pushArray(endings, {resolve, reject, value});
                closing = true;
                start();
                if (!terminal && destroyOnReturn) destroy(stream, state);
                request();
            });
        },
        throw(reason) {
            return new PromiseCtor((resolve, reject) => {
                if (terminal) { reject(reason); return; }
                pushArray(endings, {resolve, reject});
                fail(reason); closing = true;
                start();
                if (!terminal) destroy(stream, state, reason);
                request();
            });
        },
        [asyncIteratorSymbol]() { return this; },
    };
    return iterator;
}

class Readable extends EventEmitter {
    constructor(options = {}) {
        super();
        if (options === null || typeof options !== 'object') throw new TypeErrorCtor('Readable options must be an object');
        const objectMode = bool(options, 'objectMode', false);
        const highWaterMark = options.highWaterMark === undefined ? (objectMode ? 16 : 65536) : options.highWaterMark;
        if (typeof highWaterMark !== 'number') throw new TypeErrorCtor('highWaterMark must be a number');
        if (!integer(highWaterMark) || highWaterMark < 0 || highWaterMark > 67108864) throw new RangeErrorCtor('highWaterMark is out of range');
        for (const name of ['construct', 'signal', 'encoding']) if (options[name] !== undefined) throw new TypeErrorCtor(`${name} is not supported yet`);
        for (const [option, method] of [['read', '_read'], ['destroy', '_destroy']]) {
            if (options[option] !== undefined) {
                if (typeof options[option] !== 'function') throw new TypeErrorCtor(`${option} must be a function`);
                define(this, method, {value: options[option], writable: true, configurable: true});
            }
        }
        const state = {
            queue: [], pipes: [], length: 0, highWaterMark, objectMode,
            autoDestroy: bool(options, 'autoDestroy', true), emitClose: bool(options, 'emitClose', true),
            reading: false, inRead: false, scheduled: false, notifying: false, demand: false,
            flowing: null, userPaused: false, eof: false, ended: false, endScheduled: false,
            destroyed: false, closed: false, didRead: false, error: null,
        };
        weakSet(states, this, state);
        Lifecycle.register(this, 'readable', state);
    }
    _read() { this.destroy(error('_read is not implemented', 'ERR_METHOD_NOT_IMPLEMENTED')); }
    _destroy(reason, callback) { callback(reason); }
    push(chunk, encoding) {
        const state = stateOf(this);
        if (state.destroyed) return false;
        if (chunk === null) {
            if (!state.eof) { state.eof = true; state.reading = false; notify(this, state); schedule(this, state); }
            return false;
        }
        if (state.eof) { destroy(this, state, error('push after EOF', 'ERR_STREAM_PUSH_AFTER_EOF')); return false; }
        const record = normalize(state, chunk, encoding);
        if (record.size) {
            pushArray(state.queue, record);
            state.length += record.size;
            state.reading = false;
            notify(this, state);
            schedule(this, state);
        }
        return state.length < state.highWaterMark;
    }
    unshift(chunk, encoding) {
        const state = stateOf(this);
        if (state.destroyed) return false;
        if (state.ended) { destroy(this, state, error('unshift after end', 'ERR_STREAM_UNSHIFT_AFTER_END_EVENT')); return false; }
        if (chunk === null) return this.push(null);
        const record = normalize(state, chunk, encoding);
        if (record.size) { unshiftArray(state.queue, record); state.length += record.size; notify(this, state); schedule(this, state); }
        return state.length < state.highWaterMark;
    }
    read(size) {
        const state = stateOf(this);
        if (size !== undefined) {
            if (typeof size !== 'number') throw new TypeErrorCtor('read size must be a number');
            if (!integer(size) || size < 0 || size > 67108864) throw new RangeErrorCtor('read size is out of range');
            if (!state.objectMode) state.highWaterMark = max(state.highWaterMark, size);
        }
        if (state.destroyed || state.ended) return null;
        state.demand = true;
        if (size === 0) { pull(this, state); maybeEnd(this, state); return null; }
        if (!state.queue.length || (!state.objectMode && size !== undefined && state.length < size)) pull(this, state);
        if (state.destroyed) return null;
        if (!state.queue.length || (!state.objectMode && size !== undefined && state.length < size && !state.eof)) {
            schedule(this, state); maybeEnd(this, state); return null;
        }
        const count = state.objectMode ? 1 : (size === undefined ? state.length : min(size, state.length));
        return take(this, state, count);
    }
    pause() { const state = stateOf(this); state.userPaused = true; setFlow(this, state, false); return this; }
    resume() {
        const state = stateOf(this);
        state.userPaused = false; state.demand = true;
        resumePipes(this, state);
        schedule(this, state);
        return this;
    }
    isPaused() { return stateOf(this).flowing === false; }
    on(name, listener) { return subscribe(this, on, name, listener); }
    addListener(name, listener) { return subscribe(this, on, name, listener); }
    once(name, listener) { return subscribe(this, once, name, listener); }
    prependListener(name, listener) { return subscribe(this, prepend, name, listener); }
    prependOnceListener(name, listener) { return subscribe(this, prependOnce, name, listener); }
    pipe(destination, options = {}) {
        const state = stateOf(this);
        if (state.destroyed) throw error('Stream was destroyed', 'ERR_STREAM_DESTROYED');
        if (options === null || typeof options !== 'object') throw new TypeErrorCtor('pipe options must be an object');
        const end = bool(options, 'end', true);
        if (destination === null || typeof destination !== 'object' || typeof destination.write !== 'function' || typeof destination.end !== 'function') throw new TypeErrorCtor('pipe requires a writable EventEmitter');
        countListeners(destination, 'error'); // Validate emitter state before installing anything.
        const stream = this;
        const record = {destination, blocked: false, removed: false};
        record.data = chunk => {
            if (record.removed) return;
            let accepted;
            try { accepted = destination.write(chunk); }
            catch (caught) {
                detach(stream, state, record);
                if (typeof destination.destroy === 'function') destination.destroy(caught);
                else defer(() => emit(destination, 'error', caught));
                return;
            }
            if (accepted === false && !record.removed) { record.blocked = true; setFlow(stream, state, false); }
        };
        record.drain = () => { record.blocked = false; if (!record.removed) resumePipes(stream, state); };
        record.end = () => { try { if (end && !record.removed) destination.end(); } finally { detach(stream, state, record); } };
        record.close = () => detach(stream, state, record);
        record.error = reason => {
            detach(stream, state, record);
            if (countListeners(destination, 'error') === 0) emit(destination, 'error', reason);
        };
        pushArray(state.pipes, record);
        const connect = (method, target, name, listener) => {
            method(target, name, listener);
            if (record.removed || state.destroyed) throw error('Pipe setup was canceled', 'ERR_STREAM_DESTROYED');
        };
        try {
            connect(on, stream, 'data', record.data); connect(once, stream, 'end', record.end);
            connect(on, destination, 'drain', record.drain); connect(prepend, destination, 'error', record.error);
            connect(once, destination, 'finish', record.close); connect(once, destination, 'close', record.close);
            emit(destination, 'pipe', stream);
        }
        catch (caught) { detach(stream, state, record); throw caught; }
        if (record.removed) return destination;
        if (state.ended) defer(record.end);
        else this.resume();
        return destination;
    }
    unpipe(destination) {
        const state = stateOf(this);
        for (let i = state.pipes.length - 1; i >= 0; i--) if (destination === undefined || state.pipes[i].destination === destination) detach(this, state, state.pipes[i]);
        return this;
    }
    destroy(reason) { return destroy(this, stateOf(this), reason); }
    iterator(options = {}) { return createIterator(this, options); }
    [asyncIteratorSymbol]() { return createIterator(this, {}); }
    get readable() { const state = stateOf(this); return !state.destroyed && !state.ended; }
    get readableLength() { return stateOf(this).length; }
    get readableHighWaterMark() { return stateOf(this).highWaterMark; }
    get readableObjectMode() { return stateOf(this).objectMode; }
    get readableFlowing() { return stateOf(this).flowing; }
    get readableEnded() { return stateOf(this).ended; }
    get readableAborted() { const state = stateOf(this); return state.destroyed && !state.ended; }
    get readableDidRead() { return stateOf(this).didRead; }
    get readableEncoding() { stateOf(this); return null; }
    get destroyed() { return stateOf(this).destroyed; }
    get closed() { return stateOf(this).closed; }
    get errored() { return stateOf(this).error; }
    static from(iterable, options = {}) {
        if (options === null || typeof options !== 'object') throw new TypeErrorCtor('Readable.from options must be an object');
        if (options.read !== undefined || options.destroy !== undefined) throw new TypeErrorCtor('Readable.from owns read/destroy hooks');
        // Validate options before calling an iterator factory that may acquire resources.
        const source = new Readable({...options, objectMode: options.objectMode === undefined ? true : options.objectMode});
        const object = value => value !== null && (typeof value === 'object' || typeof value === 'function');
        if (typeof iterable === 'string' || isBuffer(iterable)) iterable = [iterable];
        if (iterable == null) throw new TypeErrorCtor('expected an iterable');
        const factory = iterable[asyncIteratorSymbol] ?? iterable[iteratorSymbol];
        if (typeof factory !== 'function') throw new TypeErrorCtor('expected an iterable');
        const iterator = apply(factory, iterable, []);
        if (!object(iterator)) throw new TypeErrorCtor('invalid iterator');
        const next = iterator.next;
        if (typeof next !== 'function') throw new TypeErrorCtor('invalid iterator');
        let completed = false, returned = false;
        define(source, '_read', {writable: true, configurable: true, value: function() {
            const stream = this;
            const result = apply(next, iterator, []);
            then(resolvePromise(result), item => {
                if (stream.destroyed) return;
                try {
                    if (!object(item)) throw new TypeErrorCtor('invalid iterator result');
                    if (item.done) { completed = true; stream.push(null); return; }
                    // Iterator values may themselves be Promises, as with Node's from.
                    then(resolvePromise(item.value), value => {
                        if (stream.destroyed) return;
                        try {
                            if (value === null) throw error('Null iterator values are not allowed', 'ERR_STREAM_NULL_VALUES');
                            stream.push(value);
                            const state = stateOf(stream);
                            if (!state.objectMode && state.reading) { state.reading = false; schedule(stream, state); }
                        } catch (caught) { stream.destroy(caught); }
                    }, reason => { stream.destroy(reason); });
                } catch (caught) { stream.destroy(caught); }
            }, reason => { stream.destroy(reason); });
        }});
        define(source, '_destroy', {writable: true, configurable: true, value: function(reason, done) {
            if (completed || returned) { done(reason); return; }
            returned = true;
            let result;
            try {
                const close = iterator.return;
                if (close == null) { done(reason); return; }
                if (typeof close !== 'function') throw new TypeErrorCtor('iterator.return must be a function');
                result = apply(close, iterator, []);
            } catch (caught) { done(reason ?? caught); return; }
            then(resolvePromise(result), item => {
                if (!object(item)) done(reason ?? new TypeErrorCtor('invalid iterator return result'));
                else done(reason);
            }, caught => done(reason ?? caught));
        }});
        return source;
    }
}

const readForIterator = uncurry(Readable.prototype.read);
return Readable;
