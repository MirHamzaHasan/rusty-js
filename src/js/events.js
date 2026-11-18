'use strict';

// Capture the intrinsics before guest code can replace globals or methods.
const apply = Reflect.apply;
const ownKeys = Reflect.ownKeys;
const create = Object.create;
const define = Object.defineProperty;
const NativeError = Error;
const NativeTypeError = TypeError;
const NativeRangeError = RangeError;
const string = String;
const WeakMapCtor = WeakMap;
const weakGet = WeakMap.prototype.get;
const weakSet = WeakMap.prototype.set;
const slice = Array.prototype.slice;
const push = Array.prototype.push;
const unshift = Array.prototype.unshift;
const splice = Array.prototype.splice;
const states = new WeakMapCtor();
const originals = new WeakMapCtor();
let defaultLimit = 10;

const keyOf = name => ownKeys({ [name]: true })[0];
const original = listener => apply(weakGet, originals, [listener]) || listener;
function stateOf(emitter) {
  const state = apply(weakGet, states, [emitter]);
  if (!state) throw new NativeTypeError('EventEmitter method requires an initialized emitter');
  return state;
}
function validateListener(listener) {
  if (typeof listener !== 'function') throw new NativeTypeError('listener must be a function');
}
function validateLimit(limit) {
  if (typeof limit !== 'number') throw new NativeTypeError('max listeners must be a number');
  if (!(limit >= 0)) throw new NativeRangeError('max listeners must be nonnegative');
}
function EventEmitter(options) {
  if ((typeof this !== 'object' || this === null) && typeof this !== 'function') {
    throw new NativeTypeError('EventEmitter requires an object receiver');
  }
  if (options != null) {
    if (typeof options !== 'object') throw new NativeTypeError('EventEmitter options must be an object');
    const capture = options.captureRejections;
    if (capture !== undefined && typeof capture !== 'boolean') throw new NativeTypeError('captureRejections must be a boolean');
    if (capture) throw new NativeTypeError('captureRejections is not supported');
  }
  if (!apply(weakGet, states, [this])) apply(weakSet, states, [this, { events: create(null), max: undefined }]);
}

function add(emitter, name, listener, prepend, once) {
  validateListener(listener);
  const key = keyOf(name);
  const state = stateOf(emitter);
  if (state.events.newListener) emitter.emit('newListener', key, listener);
  let callable = listener;
  if (once) {
    let fired = false;
    callable = function(...args) {
      if (fired) return;
      fired = true;
      emitter.removeListener(key, callable);
      return apply(listener, emitter, args);
    };
    define(callable, 'listener', { value: listener, writable: true, configurable: true, enumerable: true });
    apply(weakSet, originals, [callable, listener]);
  }
  // Meta-listeners may have changed this event's collection; fetch it afterward.
  const current = state.events[key] || (state.events[key] = { items: [], warned: false });
  apply(prepend ? unshift : push, current.items, [callable]);
  const limit = state.max === undefined ? defaultLimit : state.max;
  if (limit > 0 && current.items.length > limit && !current.warned) {
    current.warned = true;
    warn(`MaxListenersExceededWarning: Possible EventEmitter memory leak detected. ${current.items.length} ${string(key)} listeners added.`);
  }
  return emitter;
}
function on(name, listener) { return add(this, name, listener, false, false); }
function once(name, listener) { return add(this, name, listener, false, true); }
function prependListener(name, listener) { return add(this, name, listener, true, false); }
function prependOnceListener(name, listener) { return add(this, name, listener, true, true); }
function emit(name, ...args) {
  const key = keyOf(name);
  const current = stateOf(this).events[key];
  if (!current || current.items.length === 0) {
    if (key === 'error') {
      if (args[0] instanceof NativeError) throw args[0];
      const error = new NativeError("Unhandled 'error' event");
      define(error, 'context', { value: args[0], writable: true, configurable: true, enumerable: true });
      throw error;
    }
    return false;
  }
  const snapshot = apply(slice, current.items, []);
  for (let index = 0; index < snapshot.length; index++) apply(snapshot[index], this, args);
  return true;
}
function removeListener(name, listener) {
  validateListener(listener);
  const key = keyOf(name);
  const state = stateOf(this);
  const current = state.events[key];
  if (!current) return this;
  for (let index = current.items.length - 1; index >= 0; index--) {
    const callable = current.items[index];
    if (callable === listener || original(callable) === listener) {
      apply(splice, current.items, [index, 1]);
      if (current.items.length === 0) delete state.events[key];
      else if (current.items.length === 1) current.warned = false;
      if (state.events.removeListener) this.emit('removeListener', key, original(callable));
      break;
    }
  }
  return this;
}
function removeAllListeners(name) {
  const state = stateOf(this);
  if (!state.events.removeListener) {
    if (arguments.length > 0) delete state.events[keyOf(name)];
    else state.events = create(null);
    return this;
  }
  const removeEvent = key => {
    const current = state.events[key];
    if (!current) return;
    const snapshot = apply(slice, current.items, []);
    for (let index = snapshot.length - 1; index >= 0; index--) this.removeListener(key, snapshot[index]);
    delete state.events[key];
  };
  if (arguments.length > 0) removeEvent(keyOf(name));
  else {
    const names = ownKeys(state.events);
    for (let index = 0; index < names.length; index++) if (names[index] !== 'removeListener') removeEvent(names[index]);
    removeEvent('removeListener');
    state.events = create(null);
  }
  return this;
}
function rawListeners(name) {
  const current = stateOf(this).events[keyOf(name)];
  return current ? apply(slice, current.items, []) : [];
}
function listeners(name) {
  const result = apply(rawListeners, this, [name]);
  for (let index = 0; index < result.length; index++) result[index] = original(result[index]);
  return result;
}
function listenerCount(name, listener) {
  if (listener !== undefined) validateListener(listener);
  const current = stateOf(this).events[keyOf(name)];
  if (!current) return 0;
  if (listener === undefined) return current.items.length;
  let count = 0;
  for (let index = 0; index < current.items.length; index++) if (current.items[index] === listener || original(current.items[index]) === listener) count++;
  return count;
}
function eventNames() { return ownKeys(stateOf(this).events); }
function setMaxListeners(limit) { validateLimit(limit); stateOf(this).max = limit; return this; }
function getMaxListeners() { const state = stateOf(this); return state.max === undefined ? defaultLimit : state.max; }

for (const [name, value] of Object.entries({ on, addListener: on, once, prependListener, prependOnceListener, emit, removeListener, off: removeListener, removeAllListeners, rawListeners, listeners, listenerCount, eventNames, setMaxListeners, getMaxListeners })) {
  define(EventEmitter.prototype, name, { value, writable: true, configurable: true });
}
define(EventEmitter, 'EventEmitter', { value: EventEmitter, writable: true, configurable: true, enumerable: true });
define(EventEmitter, 'defaultMaxListeners', { get() { return defaultLimit; }, set(value) { validateLimit(value); defaultLimit = value; }, configurable: true, enumerable: true });
define(EventEmitter, 'captureRejections', { get() { return false; }, set(value) { if (typeof value !== 'boolean' || value) throw new NativeTypeError('captureRejections is not supported'); }, configurable: true });
return EventEmitter;
