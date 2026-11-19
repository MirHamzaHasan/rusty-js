'use strict';

// Private to the bundled stream implementations; never installed on guest globals.
const uncurry = fn => Function.prototype.call.bind(fn);
const get = uncurry(WeakMap.prototype.get), set = uncurry(WeakMap.prototype.set);
const push = uncurry(Array.prototype.push), splice = uncurry(Array.prototype.splice);
const indexOf = uncurry(Array.prototype.indexOf), slice = uncurry(Array.prototype.slice);
const then = uncurry(Promise.prototype.then), checkpoint = Promise.resolve();
const entries = new WeakMap();
const TypeErrorCtor = TypeError;
let errorSequence = 0;

function inspect(stream) {
    const entry = get(entries, stream);
    if (!entry) throw new TypeErrorCtor('expected a RustyJS stream');
    return entry;
}
function notify(stream) {
    const entry = inspect(stream);
    if (!entry.failure) {
        for (const kind of ['readable', 'writable']) {
            const state = entry[kind];
            if (state && state.error != null) { entry.failure = {reason: state.error, sequence: ++errorSequence}; break; }
        }
    }
    if (entry.queued || entry.observers.length === 0) return;
    entry.queued = true;
    then(checkpoint, () => {
        entry.queued = false;
        const snapshot = slice(entry.observers);
        for (let i = 0; i < snapshot.length; i++) {
            const observer = snapshot[i];
            if (observer.active) then(checkpoint, () => { if (observer.active) observer.callback(); });
        }
    });
}
function register(stream, kind, state) {
    let entry = get(entries, stream);
    if (!entry) { entry = {readable: null, writable: null, observers: [], queued: false, failure: null}; set(entries, stream, entry); }
    if (entry[kind]) throw new TypeErrorCtor('stream state already registered');
    entry[kind] = state;
}
function watch(stream, callback) {
    const entry = inspect(stream), observer = {active: true, callback};
    push(entry.observers, observer);
    notify(stream);
    return () => {
        if (!observer.active) return;
        observer.active = false;
        const index = indexOf(entry.observers, observer);
        if (index >= 0) splice(entry.observers, index, 1);
    };
}
return Object.freeze({register, inspect, watch, notify});
