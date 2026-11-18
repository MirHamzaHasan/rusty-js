'use strict';

// Capture before guest code runs. These are API stability measures, not a sandbox.
const uncurry = fn => Function.prototype.call.bind(fn);
const U8 = Uint8Array;
const AB = ArrayBuffer;
const TypeErrorCtor = TypeError;
const RangeErrorCtor = RangeError;
const integer = Number.isInteger;
const min = Math.min, max = Math.max, trunc = Math.trunc;
const arrayIsArray = Array.isArray;
const arrayBufferIsView = AB.isView;
const setPrototype = Object.setPrototypeOf;
const descriptor = Object.getOwnPropertyDescriptor;
const typedPrototype = Object.getPrototypeOf(U8.prototype);
const getLength = uncurry(descriptor(typedPrototype, 'length').get);
const getOffset = uncurry(descriptor(typedPrototype, 'byteOffset').get);
const getBuffer = uncurry(descriptor(typedPrototype, 'buffer').get);
const getByteLength = uncurry(descriptor(typedPrototype, 'byteLength').get);
const dataViewLength = uncurry(descriptor(DataView.prototype, 'byteLength').get);
const DV = DataView;
const getTag = uncurry(descriptor(typedPrototype, Symbol.toStringTag).get);
const abLength = uncurry(descriptor(AB.prototype, 'byteLength').get);
const abResizable = uncurry(descriptor(AB.prototype, 'resizable').get);
const setBytes = uncurry(U8.prototype.set);
const fillBytes = uncurry(U8.prototype.fill);
const lower = uncurry(String.prototype.toLowerCase);
const codeAt = uncurry(String.prototype.charCodeAt);
const char = String.fromCharCode;
const weakAdd = uncurry(WeakSet.prototype.add);
const weakHas = uncurry(WeakSet.prototype.has);
const brands = new WeakSet();
const token = {};
const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

function size(value, name = 'size', limit = maxLength) {
    if (typeof value !== 'number') throw new TypeErrorCtor(`${name} must be a number`);
    if (!integer(value) || value < 0 || value > limit) throw new RangeErrorCtor(`${name} is out of range`);
    return value;
}

function encoding(value) {
    if (value === undefined || value === '') return 'utf8';
    if (typeof value !== 'string') throw new TypeErrorCtor('encoding must be a string');
    switch (lower(value)) {
        case 'utf8': case 'utf-8': return 'utf8';
        case 'utf16le': case 'utf-16le': case 'ucs2': case 'ucs-2': return 'utf16le';
        case 'latin1': case 'binary': return 'latin1';
        case 'ascii': return 'ascii';
        case 'hex': return 'hex';
        case 'base64': return 'base64';
        case 'base64url': return 'base64url';
        default: throw new TypeErrorCtor(`Unknown encoding: ${value}`);
    }
}

function byteView(value) {
    if (getTag(value) !== 'Uint8Array') throw new TypeErrorCtor('expected a Buffer or Uint8Array');
    // Constructing a fresh view also rejects detached backing stores.
    return new U8(getBuffer(value), getOffset(value), getLength(value));
}

function own(value) {
    if (!weakHas(brands, value)) throw new TypeErrorCtor('incompatible Buffer receiver');
    return byteView(value);
}

function brand(view) {
    setPrototype(view, Buffer.prototype);
    weakAdd(brands, view);
    return view;
}

function hexDigit(code) {
    if (code >= 48 && code <= 57) return code - 48;
    if (code >= 65 && code <= 70) return code - 55;
    if (code >= 97 && code <= 102) return code - 87;
    return -1;
}

function base64Digit(code) {
    if (code >= 65 && code <= 90) return code - 65;
    if (code >= 97 && code <= 122) return code - 71;
    if (code >= 48 && code <= 57) return code + 4;
    if (code === 43 || code === 45) return 62;
    if (code === 47 || code === 95) return 63;
    return -1;
}

// Count first so malformed/whitespace-heavy input does not force a large buffer.
function decodedLength(string, enc) {
    if (enc === 'utf8') return utf8Length(string);
    if (enc === 'utf16le') return string.length * 2;
    if (enc === 'latin1' || enc === 'ascii') return string.length;
    if (enc === 'hex') {
        let i = 0;
        while (i + 1 < string.length && hexDigit(codeAt(string, i)) >= 0 && hexDigit(codeAt(string, i + 1)) >= 0) i += 2;
        return i / 2;
    }
    let digits = 0;
    for (let i = 0; i < string.length; i++) {
        const code = codeAt(string, i);
        if (code === 61) break;
        if (base64Digit(code) >= 0) digits++;
    }
    return trunc(digits * 6 / 8);
}

function encode(string, enc) {
    if (enc === 'utf8') return encodeUtf8(string);
    const bytes = new U8(size(decodedLength(string, enc)));
    if (enc === 'hex') {
        for (let i = 0; i < bytes.length; i++) bytes[i] = hexDigit(codeAt(string, i * 2)) * 16 + hexDigit(codeAt(string, i * 2 + 1));
    } else if (enc === 'base64' || enc === 'base64url') {
        let bits = 0, count = 0, index = 0;
        for (let i = 0; i < string.length; i++) {
            const code = codeAt(string, i);
            if (code === 61) break;
            const digit = base64Digit(code);
            if (digit < 0) continue;
            bits = (bits << 6) | digit;
            count += 6;
            if (count >= 8) { count -= 8; bytes[index++] = (bits >> count) & 255; }
        }
    } else {
        for (let i = 0; i < string.length; i++) {
            const code = codeAt(string, i);
            if (enc === 'utf16le') { bytes[i * 2] = code; bytes[i * 2 + 1] = code >> 8; }
            else bytes[i] = code; // ascii encoding, like latin1, preserves the low eight bits.
        }
    }
    return bytes;
}

function decode(bytes, enc) {
    if (enc === 'utf8') return decodeUtf8(bytes);
    let result = '';
    if (enc === 'hex') {
        const digits = '0123456789abcdef';
        for (let i = 0; i < bytes.length; i++) result += digits[bytes[i] >> 4] + digits[bytes[i] & 15];
    } else if (enc === 'base64' || enc === 'base64url') {
        for (let i = 0; i < bytes.length; i += 3) {
            const a = bytes[i], b = bytes[i + 1], c = bytes[i + 2];
            const digit = n => enc === 'base64url' ? (n === 62 ? '-' : n === 63 ? '_' : alphabet[n]) : alphabet[n];
            result += digit(a >> 2) + digit(((a & 3) << 4) | ((b ?? 0) >> 4));
            if (b !== undefined) result += digit(((b & 15) << 2) | ((c ?? 0) >> 6));
            else if (enc === 'base64') result += '=';
            if (c !== undefined) result += digit(c & 63);
            else if (enc === 'base64') result += '=';
        }
    } else {
        const step = enc === 'utf16le' ? 2 : 1;
        for (let i = 0; i + step <= bytes.length; i += step) {
            result += char(step === 2 ? bytes[i] | bytes[i + 1] << 8 : bytes[i] & (enc === 'ascii' ? 127 : 255));
        }
    }
    return result;
}

function relative(value, length, fallback) {
    if (value === undefined) return fallback;
    const n = +value;
    const index = n !== n ? 0 : trunc(n);
    return index < 0 ? max(length + index, 0) : min(index, length);
}

function view(bytes, start, end) {
    return new U8(getBuffer(bytes), getOffset(bytes) + start, max(0, end - start));
}

class Buffer extends U8 {
    constructor(key, length) {
        if (key !== token) throw new TypeErrorCtor('Use Buffer.from() or Buffer.alloc()');
        super(length);
        weakAdd(brands, this);
    }
    static get [Symbol.species]() { return U8; }
    static alloc(length, fill, enc) {
        const buffer = new Buffer(token, size(length));
        if (fill !== undefined) buffer.fill(fill, 0, length, enc);
        return buffer;
    }
    static allocUnsafe(length) { return Buffer.alloc(length); }
    static allocUnsafeSlow(length) { return Buffer.alloc(length); }
    static from(value, offsetOrEncoding, length) {
        if (typeof value === 'string') return brand(encode(value, encoding(offsetOrEncoding)));
        let byteLength;
        try { byteLength = abLength(value); } catch { /* Not an ArrayBuffer. */ }
        if (byteLength !== undefined) {
            if (abResizable(value)) throw new TypeErrorCtor('Resizable ArrayBuffers are not supported');
            const offset = offsetOrEncoding === undefined ? 0 : size(offsetOrEncoding, 'byteOffset', byteLength);
            const count = length === undefined ? byteLength - offset : size(length, 'length', byteLength - offset);
            size(count);
            return brand(new U8(value, offset, count));
        }
        if (value !== null && typeof value === 'object') {
            if (value.type === 'Buffer' && arrayIsArray(value.data)) value = value.data;
            // Array-likes copy element values (not raw multibyte storage).
            const count = size(value.length, 'length');
            const result = new Buffer(token, count);
            for (let i = 0; i < count; i++) result[i] = value[i];
            return result;
        }
        throw new TypeErrorCtor('Buffer.from expects a string, ArrayBuffer, or array-like object');
    }
    static isBuffer(value) { return weakHas(brands, value); }
    static isEncoding(value) {
        if (typeof value !== 'string' || value === '') return false;
        try { encoding(value); return true; } catch { return false; }
    }
    static byteLength(value, enc) {
        if (typeof value === 'string') return decodedLength(value, encoding(enc));
        if (arrayBufferIsView(value)) {
            try { return getByteLength(value); } catch { return dataViewLength(value); }
        }
        try { return abLength(value); } catch { throw new TypeErrorCtor('expected string, ArrayBuffer, or view'); }
    }
    static compare(a, b) {
        a = byteView(a); b = byteView(b);
        for (let i = 0; i < min(a.length, b.length); i++) if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
        return a.length === b.length ? 0 : a.length < b.length ? -1 : 1;
    }
    static concat(list, length) {
        if (!arrayIsArray(list)) throw new TypeErrorCtor('list must be an array');
        const views = [];
        let total = 0;
        for (let i = 0; i < list.length; i++) { const bytes = byteView(list[i]); views[i] = bytes; total += bytes.length; }
        const result = Buffer.alloc(length === undefined ? size(total) : size(length));
        let offset = 0;
        for (let i = 0; i < views.length && offset < result.length; i++) {
            const count = min(views[i].length, result.length - offset);
            setBytes(result, view(views[i], 0, count), offset); offset += count;
        }
        return result;
    }
    subarray(start, end) {
        const bytes = own(this), length = bytes.length;
        start = relative(start, length, 0); end = relative(end, length, length);
        return brand(view(bytes, start, end));
    }
    slice(start, end) {
        const bytes = own(this), length = bytes.length;
        return brand(view(bytes, relative(start, length, 0), relative(end, length, length)));
    }
    toString(enc, start = 0, end) {
        const bytes = own(this);
        enc = encoding(enc);
        const clamp = value => { const n = +value; return n !== n ? 0 : min(bytes.length, max(0, trunc(n))); };
        start = clamp(start); end = end === undefined ? bytes.length : clamp(end);
        return decode(view(bytes, start, end), enc);
    }
    toJSON() {
        const bytes = own(this), data = [];
        for (let i = 0; i < bytes.length; i++) data[i] = bytes[i];
        return {type: 'Buffer', data};
    }
    equals(other) { return Buffer.compare(own(this), other) === 0; }
    compare(other) { return Buffer.compare(own(this), other); }
    copy(target, targetStart = 0, sourceStart = 0, sourceEnd) {
        const source = own(this); target = byteView(target);
        size(targetStart, 'targetStart', target.length);
        size(sourceStart, 'sourceStart', source.length);
        sourceEnd = sourceEnd === undefined ? source.length : size(sourceEnd, 'sourceEnd', source.length);
        const count = min(max(0, sourceEnd - sourceStart), target.length - targetStart);
        setBytes(target, view(source, sourceStart, sourceStart + count), targetStart);
        return count;
    }
    fill(value, offset = 0, end, enc) {
        const bytes = own(this);
        if (typeof offset === 'string') { enc = offset; offset = 0; end = bytes.length; }
        else if (typeof end === 'string') { enc = end; end = bytes.length; }
        size(offset, 'offset', bytes.length); end = end === undefined ? bytes.length : size(end, 'end', bytes.length);
        if (typeof value === 'number') { fillBytes(bytes, value, offset, end); return this; }
        const pattern = typeof value === 'string' ? encode(value, encoding(enc)) : byteView(value);
        if (pattern.length === 0) {
            if (typeof value === 'string' && value === '') { fillBytes(bytes, 0, offset, end); return this; }
            throw new TypeErrorCtor('fill pattern must not be empty');
        }
        // Snapshot first: the fill pattern may alias the destination.
        const copy = new U8(size(pattern.length)); setBytes(copy, pattern);
        for (let i = offset; i < end; i++) bytes[i] = copy[(i - offset) % copy.length];
        return this;
    }
    write(string, offset = 0, length, enc) {
        const target = own(this);
        if (typeof string !== 'string') throw new TypeErrorCtor('string must be a string');
        if (typeof offset === 'string') { enc = offset; offset = 0; length = target.length; }
        else if (typeof length === 'string') { enc = length; length = undefined; }
        size(offset, 'offset', target.length);
        length = length === undefined ? target.length - offset : min(size(length, 'length'), target.length - offset);
        enc = encoding(enc);
        const bytes = encode(string, enc);
        let count = min(length, bytes.length);
        if (enc === 'utf8' && count < bytes.length) {
            // Never write part of a UTF-8 codepoint.
            while (count > 0 && (bytes[count] & 192) === 128) count--;
        } else if (enc === 'utf16le') count -= count % 2;
        setBytes(target, view(bytes, 0, count), offset);
        return count;
    }
}

// Fixed-width numeric operations use V8's DataView with explicit type/range checks.
for (const [name, suffix, width, low, high, kind] of [
    ['UInt8', 'Uint8', 1, 0, 255, 'integer'],
    ['Int8', 'Int8', 1, -128, 127, 'integer'],
    ['UInt16', 'Uint16', 2, 0, 65535, 'integer'],
    ['Int16', 'Int16', 2, -32768, 32767, 'integer'],
    ['UInt32', 'Uint32', 4, 0, 4294967295, 'integer'],
    ['Int32', 'Int32', 4, -2147483648, 2147483647, 'integer'],
    ['Float', 'Float32', 4, 0, 0, 'float'],
    ['Double', 'Float64', 8, 0, 0, 'float'],
    ['BigUInt64', 'BigUint64', 8, 0n, 18446744073709551615n, 'bigint'],
    ['BigInt64', 'BigInt64', 8, -9223372036854775808n, 9223372036854775807n, 'bigint'],
]) {
    const get = uncurry(DV.prototype['get' + suffix]);
    const set = uncurry(DV.prototype['set' + suffix]);
    for (const endian of width === 1 ? [''] : ['LE', 'BE']) {
        const read = function(offset = 0) {
            const bytes = own(this);
            size(offset, 'offset', bytes.length - width);
            return get(new DV(getBuffer(bytes), getOffset(bytes), bytes.length), offset, endian === 'LE');
        };
        const write = function(value, offset = 0) {
            const bytes = own(this);
            if (typeof value !== (kind === 'bigint' ? 'bigint' : 'number')) throw new TypeErrorCtor('invalid numeric value');
            if (kind !== 'float' && ((kind === 'integer' && !integer(value)) || value < low || value > high)) throw new RangeErrorCtor('value is out of range');
            size(offset, 'offset', bytes.length - width);
            set(new DV(getBuffer(bytes), getOffset(bytes), bytes.length), offset, value, endian === 'LE');
            return offset + width;
        };
        Object.defineProperty(Buffer.prototype, 'read' + name + endian, {value: read, writable: true, configurable: true});
        Object.defineProperty(Buffer.prototype, 'write' + name + endian, {value: write, writable: true, configurable: true});
        if (name === 'UInt8' || name === 'UInt16' || name === 'UInt32' || name === 'BigUInt64') {
            const alias = name === 'BigUInt64' ? 'BigUint64' : 'Uint' + width * 8;
            Object.defineProperty(Buffer.prototype, 'read' + alias + endian, {value: read, writable: true, configurable: true});
            Object.defineProperty(Buffer.prototype, 'write' + alias + endian, {value: write, writable: true, configurable: true});
        }
    }
}

return Buffer;
