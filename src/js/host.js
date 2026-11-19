'use strict';

const uncurry = fn => Function.prototype.call.bind(fn);
const define = Object.defineProperty, create = Object.create, freeze = Object.freeze;
const entries = Object.entries, includes = uncurry(String.prototype.includes);
const slice = uncurry(String.prototype.slice), bigint = BigInt;
const TypeErrorCtor = TypeError, RangeErrorCtor = RangeError;
const string = String, number = Number, array = Array.isArray, safe = Number.isSafeInteger;
const environment = create(null);
function nameOf(key) {
    if (typeof key !== 'string' || key === '' || includes(key, '=') || includes(key, '\0')) throw new TypeErrorCtor('invalid environment variable name');
    return key;
}
function text(value) {
    if (typeof value === 'symbol') throw new TypeErrorCtor('environment values cannot be Symbols');
    return string(value);
}
function store(key, value) {
    define(environment, nameOf(key), {value: text(value), enumerable: true, configurable: true, writable: true});
}
for (const [key, value] of entries(info.env)) store(key, value);
const env = new Proxy(environment, {
    set(target, key, value) { store(key, value); return true; },
    defineProperty(target, key, descriptor) {
        if (!('value' in descriptor) || descriptor.get || descriptor.set || descriptor.writable !== true || descriptor.enumerable !== true || descriptor.configurable !== true) throw new TypeErrorCtor('environment properties must be ordinary mutable data properties');
        store(key, descriptor.value); return true;
    },
    setPrototypeOf(target, prototype) { if (prototype !== null) throw new TypeErrorCtor('environment prototype must remain null'); return true; },
    preventExtensions() { throw new TypeErrorCtor('environment view must remain extensible'); },
});

function hrtime(previous) {
    let time = elapsed();
    if (previous !== undefined) {
        if (!array(previous) || previous.length !== 2) throw new TypeErrorCtor('hrtime requires a [seconds, nanoseconds] array');
        const seconds = previous[0], nanos = previous[1];
        if (!safe(seconds) || seconds < 0 || !safe(nanos) || nanos < 0 || nanos >= 1000000000) throw new RangeErrorCtor('hrtime components are out of range');
        time -= bigint(seconds) * 1000000000n + bigint(nanos);
    }
    let seconds = time / 1000000000n, nanos = time % 1000000000n;
    if (nanos < 0n) { seconds--; nanos += 1000000000n; }
    return [number(seconds), number(nanos)];
}
define(hrtime, 'bigint', {value: elapsed, enumerable: true});
const process = {argv: info.argv, execArgv: info.execArgv, env, cwd, uptime: () => number(elapsed()) / 1000000000, hrtime};
for (const key of ['argv0','execPath','platform','arch','pid','version']) define(process,key,{value:info[key],enumerable:true});
define(process,'versions',{value:freeze(info.versions),enumerable:true});
define(process,'release',{value:freeze({name:'rustyjs'}),enumerable:true});
define(process,'exitCode',{enumerable:true,get(){return status();},set(value){status(value);}});

const os = {
    platform: () => info.platform,
    arch: () => info.arch,
    type: () => info.osType,
    endianness: () => info.endianness,
    availableParallelism: parallelism,
    homedir: home,
    tmpdir() {
        const path = temp();
        // Keep roots intact; strip trailing separators on ordinary temp locations.
        let end = path.length;
        const rootLength = info.platform === 'win32' && path[1] === ':' ? 3 : 1;
        while (end > rootLength && (path[end-1] === '/' || (info.platform === 'win32' && path[end-1] === '\\'))) end--;
        return slice(path,0,end);
    },
    EOL: info.EOL,
    devNull: info.devNull,
};
return {process, os};
