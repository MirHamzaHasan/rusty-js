'use strict';

const uncurry = fn => Function.prototype.call.bind(fn);
const prepend = uncurry(EventEmitter.prototype.prependListener);
const remove = uncurry(EventEmitter.prototype.removeListener);
const pipe = uncurry(Readable.prototype.pipe), unpipe = uncurry(Readable.prototype.unpipe);
const destroyReadable = uncurry(Readable.prototype.destroy), destroyWritable = uncurry(Writable.prototype.destroy);
const endWritable = uncurry(Writable.prototype.end), from = Readable.from;
const then = uncurry(Promise.prototype.then), checkpoint = Promise.resolve();
const defer = fn => { then(checkpoint, () => { fn(); }); };
const PromiseCtor = Promise, TypeErrorCtor = TypeError, ErrorCtor = Error;
const isArray = Array.isArray;

function error(message, code) { const reason = new ErrorCtor(message); reason.code = code; return reason; }
function optionsOf(options) {
    if (options === null || typeof options !== 'object') throw new TypeErrorCtor('options must be an object');
    if (options.signal !== undefined) throw new TypeErrorCtor('AbortSignal is not supported yet');
    return options;
}
function bool(options, name, fallback) {
    const value = options[name];
    if (value === undefined) return fallback;
    if (typeof value !== 'boolean') throw new TypeErrorCtor(`${name} must be a boolean`);
    return value;
}
function cleanError(stream, listener) {
    try { remove(stream, 'error', listener); }
    catch (caught) { defer(() => { throw caught; }); }
}
function ready(state, done) { return state.closed || (done && !state.autoDestroy && !state.destroyed); }

function finished(stream, options, callback) {
    if (typeof options === 'function') { callback = options; options = {}; }
    options = optionsOf(options === undefined ? {} : options);
    if (typeof callback !== 'function') throw new TypeErrorCtor('finished requires a callback');
    const entry = Lifecycle.inspect(stream);
    const read = bool(options, 'readable', true) && entry.readable;
    const write = bool(options, 'writable', true) && entry.writable;
    const errors = bool(options, 'error', true), autoCleanup = bool(options, 'cleanup', false);
    let active = true, settled = false, unwatch, manualFailure = null;
    const cleanup = () => {
        active = false;
        callback = undefined;
        if (unwatch) { unwatch(); unwatch = undefined; }
        if (errors) cleanError(stream, onError);
    };
    const complete = reason => {
        if (!active || settled) return;
        settled = true;
        if (unwatch) { unwatch(); unwatch = undefined; }
        const notify = callback; callback = undefined;
        if (autoCleanup) cleanup();
        defer(() => notify(reason));
    };
    function check() {
        if (!active || settled) return;
        const failure = errors && (entry.failure || manualFailure);
        // Registered destruction must finish cleanup before reporting its result.
        if ((read && read.destroyed && !read.closed) || (write && write.destroyed && !write.closed)) return;
        if (failure) { complete(failure.reason); return; }
        if ((read && read.closed && !read.ended) || (write && write.closed && !write.finished)) {
            complete(error('Stream closed before completion', 'ERR_STREAM_PREMATURE_CLOSE')); return;
        }
        if ((!read || (read.ended && ready(read, true))) && (!write || (write.finished && ready(write, true)))) complete();
    }
    function onError(reason) { if (!manualFailure) manualFailure = {reason}; check(); }
    unwatch = Lifecycle.watch(stream, check);
    try { if (errors) prepend(stream, 'error', onError); }
    catch (caught) { cleanup(); throw caught; }
    return cleanup;
}
function finishedPromise(stream, options) {
    return new PromiseCtor((resolve, reject) => {
        finished(stream, options, reason => { if (reason != null) reject(reason); else resolve(); });
    });
}

function argumentsOf(args, callbackForm) {
    const values = [...args];
    const callback = callbackForm ? values.pop() : undefined;
    if (callbackForm && typeof callback !== 'function') throw new TypeErrorCtor('pipeline requires a callback');
    let options = {};
    // An array followed by a registered Writable is an iterable source, whereas
    // a single endpoint array (optionally followed by options) is the list form.
    let positionalDestination = false;
    if (values.length >= 2) {
        try { positionalDestination = !!Lifecycle.inspect(values[1]).writable; } catch { /* Not an endpoint. */ }
    }
    if (isArray(values[0]) && !positionalDestination) {
        const endpoints = values.shift();
        if (values.length > 1) throw new TypeErrorCtor('invalid pipeline arguments');
        if (values.length) options = values[0];
        if (endpoints.length !== 2) throw new TypeErrorCtor('pipeline currently requires a source and a destination');
        return {source: endpoints[0], destination: endpoints[1], options: optionsOf(options), callback};
    }
    if (values.length < 2 || values.length > 3) throw new TypeErrorCtor('pipeline currently requires a source and a destination');
    if (values.length === 3) options = values[2];
    return {source: values[0], destination: values[1], options: optionsOf(options), callback};
}
function runPipeline(source, destination, options, callback) {
    const end = bool(options, 'end', true);
    const destinationEntry = Lifecycle.inspect(destination), writable = destinationEntry.writable;
    if (!writable) throw new TypeErrorCtor('pipeline destination must be Writable');
    let sourceEntry;
    try { sourceEntry = Lifecycle.inspect(source); }
    catch { source = from(source); sourceEntry = Lifecycle.inspect(source); }
    if (!sourceEntry.readable) throw new TypeErrorCtor('pipeline source must be Readable or iterable');
    const readable = sourceEntry.readable;
    let active = true, failed = false, failure, aborting = false, setup = true;
    const unwatch = [];
    const fail = reason => { if (!failed) { failed = true; failure = reason; } };
    const firstFailure = () => {
        const a = sourceEntry.failure, b = destinationEntry.failure;
        return a && b ? (a.sequence < b.sequence ? a : b) : a || b;
    };
    const sourceError = reason => { fail(firstFailure()?.reason ?? reason); check(); };
    const destinationError = reason => { fail(firstFailure()?.reason ?? reason); check(); };
    function cleanup() {
        active = false;
        for (let i = 0; i < unwatch.length; i++) unwatch[i]();
        cleanError(source, sourceError); cleanError(destination, destinationError);
    }
    function abort() {
        if (aborting) return;
        aborting = true;
        unpipe(source, destination);
        if (!readable.ended && !readable.destroyed) destroyReadable(source, failure);
        if (!writable.finished && !writable.destroyed) destroyWritable(destination, failure);
    }
    function complete() {
        cleanup();
        defer(() => callback(failed ? failure : undefined));
    }
    function check() {
        if (!active || setup) return;
        const recorded = firstFailure();
        if (recorded) fail(recorded.reason);
        if (!failed && ((readable.closed && !readable.ended) || (writable.closed && !writable.finished) || (writable.finished && !readable.ended))) {
            fail(error('Pipeline closed before completion', 'ERR_STREAM_PREMATURE_CLOSE'));
        }
        if (failed) {
            abort();
            if (ready(readable, readable.ended) && ready(writable, writable.finished)) complete();
            return;
        }
        const sourceDone = readable.ended && ready(readable, true);
        const destinationDone = end
            ? (writable.finished && ready(writable, true))
            : ((writable.destroyed || writable.ending || writable.finished)
                ? ready(writable, writable.finished)
                : (!writable.active && !writable.completing && writable.length === 0));
        if (sourceDone && destinationDone) complete();
    }
    // Observation is transactional; ownership begins only once both error guards
    // are installed. A metadata exception during setup leaves no helper bindings.
    try {
        unwatch[0] = Lifecycle.watch(source, check); unwatch[1] = Lifecycle.watch(destination, check);
        prepend(source, 'error', sourceError); prepend(destination, 'error', destinationError);
    } catch (caught) { cleanup(); throw caught; }
    setup = false;
    const existingFailure = firstFailure();
    if (existingFailure) fail(existingFailure.reason);
    try {
        if (!failed && !readable.destroyed && !readable.ended) pipe(source, destination, {end});
        else if (!failed && readable.ended && end && !writable.destroyed) endWritable(destination);
    } catch (caught) { fail(caught); }
    check();
    return destination;
}
function pipeline(...args) {
    const parsed = argumentsOf(args, true);
    return runPipeline(parsed.source, parsed.destination, parsed.options, parsed.callback);
}
function pipelinePromise(...args) {
    return new PromiseCtor((resolve, reject) => {
        const parsed = argumentsOf(args, false);
        runPipeline(parsed.source, parsed.destination, parsed.options, reason => { if (reason != null) reject(reason); else resolve(); });
    });
}
const promises = {finished: finishedPromise, pipeline: pipelinePromise};
return {finished, pipeline, promises};
