'use strict';

const uncurry = fn => Function.prototype.call.bind(fn);
const apply = Reflect.apply;
const TypeErrorCtor = TypeError, RangeErrorCtor = RangeError, ErrorCtor = Error;
const integer = Number.isInteger;
const define = Object.defineProperty;
const weakGet = uncurry(WeakMap.prototype.get), weakSet = uncurry(WeakMap.prototype.set);
const push = uncurry(Array.prototype.push), shift = uncurry(Array.prototype.shift);
const then = uncurry(Promise.prototype.then);
const checkpoint = Promise.resolve();
const defer = fn => { then(checkpoint, () => { fn(); }); };
const states = new WeakMap();
const bufferFrom = Buffer.from, isBuffer = Buffer.isBuffer, isEncoding = Buffer.isEncoding;
const typedPrototype = Object.getPrototypeOf(Uint8Array.prototype);
const typedTag = uncurry(Object.getOwnPropertyDescriptor(typedPrototype, Symbol.toStringTag).get);
const byteLength = uncurry(Object.getOwnPropertyDescriptor(typedPrototype, 'byteLength').get);
const validateBytes = uncurry(Uint8Array.prototype.values);
const emit = uncurry(EventEmitter.prototype.emit);
const noop = () => {};

function stateOf(stream) {
    const state = weakGet(states, stream);
    if (!state) throw new TypeErrorCtor('incompatible Writable receiver');
    return state;
}
function error(message, code) { const result = new ErrorCtor(message); result.code = code; return result; }
function validateCallback(callback) {
    if (callback !== undefined && typeof callback !== 'function') throw new TypeErrorCtor('callback must be a function');
    return callback || noop;
}
function optionBoolean(options, name, fallback) {
    const value = options[name];
    if (value === undefined) return fallback;
    if (typeof value !== 'boolean') throw new TypeErrorCtor(`${name} must be a boolean`);
    return value;
}
function validateEncoding(encoding) {
    if (!isEncoding(encoding)) throw new TypeErrorCtor('unsupported stream encoding');
    return encoding;
}
function settle(record, reason) {
    if (record.settled) return;
    record.settled = true;
    const callback = record.callback;
    defer(() => callback(reason));
}
function finishDestroy(stream, state, reason) {
    if (reason != null) state.error = reason;
    if (state.error != null && !state.errorEmitted) {
        state.errorEmitted = true;
        defer(() => emit(stream, 'error', state.error));
    }
    defer(() => {
        state.closed = true;
        Lifecycle.notify(stream);
        if (state.emitClose) emit(stream, 'close');
    });
}
function destroy(stream, state, reason) {
    if (state.destroyed) return stream;
    state.destroyed = true;
    if (reason != null) state.error = reason;
    const pendingError = state.error ?? error('Stream was destroyed', 'ERR_STREAM_DESTROYED');
    if (state.active) settle(state.active, pendingError);
    state.active = null;
    for (let i = 0; i < state.queue.length; i++) settle(state.queue[i], pendingError);
    state.queue = [];
    state.length = 0;
    state.needDrain = false;
    state.corked = 0;
    for (let i = 0; i < state.endCallbacks.length; i++) {
        const callback = state.endCallbacks[i];
        defer(() => callback(pendingError));
    }
    state.endCallbacks = [];
    Lifecycle.notify(stream);
    let called = false;
    const done = result => {
        if (called) return; // Destruction is idempotent even for a faulty cleanup hook.
        called = true;
        defer(() => finishDestroy(stream, state, result));
    };
    try { apply(stream._destroy, stream, [state.error, done]); }
    catch (caught) { if (called) state.error = caught; else done(caught); }
    return stream;
}
function finalize(stream, state) {
    if (state.finalizing || state.finished || state.destroyed) return;
    state.finalizing = true;
    let called = false;
    const done = reason => {
        if (called) { destroy(stream, state, state.error ?? error('Callback called more than once', 'ERR_MULTIPLE_CALLBACK')); return; }
        called = true;
        if (reason != null && state.error === null) { state.error = reason; Lifecycle.notify(stream); }
        defer(() => {
            if (state.destroyed) return;
            if (reason != null) { destroy(stream, state, reason); return; }
            defer(() => { if (!state.destroyed) emit(stream, 'prefinish'); });
            defer(() => {
                if (state.destroyed) return;
                state.finished = true;
                Lifecycle.notify(stream);
                // A listener exception must not prevent end callbacks or cleanup.
                try { emit(stream, 'finish'); }
                finally {
                    for (let i = 0; i < state.endCallbacks.length; i++) {
                        const callback = state.endCallbacks[i];
                        defer(() => callback());
                    }
                    state.endCallbacks = [];
                    if (state.autoDestroy) defer(() => destroy(stream, state));
                }
            });
        });
    };
    try { apply(stream._final, stream, [done]); }
    catch (caught) { if (called) destroy(stream, state, caught); else done(caught); }
}
function pump(stream, state) {
    if (state.destroyed || state.active || state.completing || state.corked) return;
    const record = shift(state.queue);
    if (!record) { if (state.ending) finalize(stream, state); return; }
    state.active = record;
    let called = false;
    const done = reason => {
        if (state.destroyed) return;
        if (called) { destroy(stream, state, state.error ?? error('Callback called more than once', 'ERR_MULTIPLE_CALLBACK')); return; }
        called = true;
        if (reason != null && state.error === null) { state.error = reason; Lifecycle.notify(stream); }
        defer(() => {
            if (state.destroyed || state.active !== record) return;
            if (reason != null) { destroy(stream, state, reason); return; }
            state.active = null;
            state.completing = true;
            state.length -= record.size;
            Lifecycle.notify(stream);
            if (state.needDrain && state.length === 0 && !state.ending) {
                defer(() => {
                    if (!state.destroyed && !state.ending && state.length === 0 && state.needDrain) {
                        state.needDrain = false;
                        emit(stream, 'drain');
                    }
                });
            }
            settle(record);
            defer(() => { state.completing = false; Lifecycle.notify(stream); pump(stream, state); });
        });
    };
    try { apply(stream._write, stream, [record.chunk, record.encoding, done]); }
    catch (caught) { if (called) destroy(stream, state, caught); else done(caught); }
}

class Writable extends EventEmitter {
    constructor(options = {}) {
        super();
        if (options === null || typeof options !== 'object') throw new TypeErrorCtor('Writable options must be an object');
        const objectMode = optionBoolean(options, 'objectMode', false);
        const highWaterMark = options.highWaterMark === undefined ? (objectMode ? 16 : 65536) : options.highWaterMark;
        if (typeof highWaterMark !== 'number') throw new TypeErrorCtor('highWaterMark must be a number');
        if (!integer(highWaterMark) || highWaterMark < 0 || highWaterMark > 67108864) throw new RangeErrorCtor('highWaterMark is out of range');
        for (const name of ['construct', 'writev', 'signal']) if (options[name] !== undefined) throw new TypeErrorCtor(`${name} is not supported yet`);
        for (const [option, method] of [['write', '_write'], ['final', '_final'], ['destroy', '_destroy']]) {
            if (options[option] !== undefined) {
                if (typeof options[option] !== 'function') throw new TypeErrorCtor(`${option} must be a function`);
                define(this, method, {value: options[option], writable: true, configurable: true});
            }
        }
        const state = {
            queue: [], active: null, completing: false, length: 0, highWaterMark, objectMode,
            decodeStrings: optionBoolean(options, 'decodeStrings', true),
            defaultEncoding: options.defaultEncoding === undefined ? 'utf8' : validateEncoding(options.defaultEncoding),
            autoDestroy: optionBoolean(options, 'autoDestroy', true),
            emitClose: optionBoolean(options, 'emitClose', true),
            corked: 0, needDrain: false, ending: false, finalizing: false, finished: false,
            destroyed: false, closed: false, error: null, errorEmitted: false, endCallbacks: [],
        };
        weakSet(states, this, state);
        Lifecycle.register(this, 'writable', state);
    }
    _write(chunk, encoding, callback) { callback(error('_write is not implemented', 'ERR_METHOD_NOT_IMPLEMENTED')); }
    _final(callback) { callback(); }
    _destroy(reason, callback) { callback(reason); }
    write(chunk, encoding, callback) {
        const state = stateOf(this);
        if (typeof encoding === 'function') { callback = encoding; encoding = undefined; }
        callback = validateCallback(callback);
        encoding = encoding === undefined ? state.defaultEncoding : validateEncoding(encoding);
        if (chunk === null || (!state.objectMode && chunk === undefined)) throw new TypeErrorCtor('invalid writable chunk');
        let length = 1;
        if (!state.objectMode) {
            if (typeof chunk === 'string') {
                if (state.decodeStrings) { chunk = bufferFrom(chunk, encoding); encoding = 'buffer'; length = byteLength(chunk); }
                else length = chunk.length;
            } else {
                if (typedTag(chunk) !== 'Uint8Array') throw new TypeErrorCtor('byte stream chunks must be strings, Buffer, or Uint8Array');
                validateBytes(chunk);
                if (!isBuffer(chunk)) chunk = bufferFrom(chunk);
                else byteLength(chunk); // Validate the storage using captured intrinsics.
                length = byteLength(chunk);
                encoding = 'buffer';
            }
        }
        if (state.destroyed || state.ending) {
            const reason = state.destroyed ? (state.error ?? error('Stream was destroyed', 'ERR_STREAM_DESTROYED')) : error('write after end', 'ERR_STREAM_WRITE_AFTER_END');
            defer(() => callback(reason));
            if (!state.destroyed) destroy(this, state, reason);
            return false;
        }
        const record = {chunk, encoding, callback, size: length, settled: false};
        push(state.queue, record);
        state.length += length;
        const accepted = state.length < state.highWaterMark;
        if (!accepted) state.needDrain = true;
        pump(this, state);
        return accepted;
    }
    end(chunk, encoding, callback) {
        const state = stateOf(this);
        if (typeof chunk === 'function') { callback = chunk; chunk = undefined; encoding = undefined; }
        else if (typeof encoding === 'function') { callback = encoding; encoding = undefined; }
        validateCallback(callback);
        if (chunk !== undefined) this.write(chunk, encoding);
        if (callback !== undefined) {
            if (state.finished) defer(() => callback());
            else if (state.destroyed) defer(() => callback(state.error ?? error('Stream was destroyed', 'ERR_STREAM_DESTROYED')));
            else push(state.endCallbacks, callback);
        }
        state.ending = true;
        state.corked = 0;
        defer(() => pump(this, state));
        return this;
    }
    cork() { stateOf(this).corked++; }
    uncork() { const state = stateOf(this); if (state.corked > 0) state.corked--; pump(this, state); }
    setDefaultEncoding(encoding) { stateOf(this).defaultEncoding = validateEncoding(encoding); return this; }
    destroy(reason) { return destroy(this, stateOf(this), reason); }
    get writable() { const s = stateOf(this); return !s.destroyed && !s.ending; }
    get writableLength() { return stateOf(this).length; }
    get writableHighWaterMark() { return stateOf(this).highWaterMark; }
    get writableObjectMode() { return stateOf(this).objectMode; }
    get writableNeedDrain() { return stateOf(this).needDrain; }
    get writableCorked() { return stateOf(this).corked; }
    get writableEnded() { return stateOf(this).ending; }
    get writableFinished() { return stateOf(this).finished; }
    get writableAborted() { const s = stateOf(this); return s.destroyed && !s.finished; }
    get destroyed() { return stateOf(this).destroyed; }
    get closed() { return stateOf(this).closed; }
    get errored() { return stateOf(this).error; }
}

return Writable;
