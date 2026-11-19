'use strict';

const uncurry = fn => Function.prototype.call.bind(fn);
const slice = uncurry(String.prototype.slice), split = uncurry(String.prototype.split);
const lower = uncurry(String.prototype.toLowerCase), charCode = uncurry(String.prototype.charCodeAt);
const lastIndex = uncurry(String.prototype.lastIndexOf);
const joinArray = uncurry(Array.prototype.join), push = uncurry(Array.prototype.push), pop = uncurry(Array.prototype.pop);
const isArray = Array.isArray;
const TypeErrorCtor = TypeError;
const create = Object.create;
const string = value => { if (typeof value !== 'string') throw new TypeErrorCtor('path must be a string'); return value; };
const drive = value => {
    const code = charCode(value, 0);
    return ((code >= 65 && code <= 90) || (code >= 97 && code <= 122)) && value[1] === ':';
};

function variant(windows) {
    const sep = windows ? '\\' : '/';
    const separator = value => value === '/' || (windows && value === '\\');
    function root(value) {
        if (windows && drive(value)) return {end: separator(value[2]) ? 3 : 2, absolute: separator(value[2]), device: slice(value, 0, 2)};
        if (!separator(value[0])) return {end: 0, absolute: false, device: ''};
        if (windows && separator(value[1]) && value.length > 2 && !separator(value[2])) {
            let serverEnd = 2;
            while (serverEnd < value.length && !separator(value[serverEnd])) serverEnd++;
            let shareStart = serverEnd;
            while (shareStart < value.length && separator(value[shareStart])) shareStart++;
            let shareEnd = shareStart;
            while (shareEnd < value.length && !separator(value[shareEnd])) shareEnd++;
            if (shareStart < shareEnd) return {
                end: shareEnd < value.length ? shareEnd + 1 : shareEnd,
                absolute: true,
                device: '\\\\' + slice(value, 2, serverEnd) + '\\' + slice(value, shareStart, shareEnd),
            };
        }
        return {end: 1, absolute: true, device: ''};
    }
    function components(value, start = 0) {
        const result = [];
        let beginning = start;
        for (let i = start; i <= value.length; i++) {
            if (i === value.length || separator(value[i])) {
                if (beginning < i) push(result, slice(value, beginning, i));
                beginning = i + 1;
            }
        }
        return result;
    }
    function tail(value, absolute) {
        const input = components(value), result = [];
        for (let i = 0; i < input.length; i++) {
            const part = input[i];
            if (part === '.') continue;
            if (part === '..') {
                if (result.length && result[result.length - 1] !== '..') pop(result);
                else if (!absolute) push(result, part);
            } else push(result, part);
        }
        return joinArray(result, sep);
    }
    function normalize(path) {
        string(path);
        if (path === '') return '.';
        const info = root(path);
        let body = tail(slice(path, info.end), info.absolute);
        if (!body && !info.absolute) body = '.';
        if (body && separator(path[path.length - 1])) body += sep;
        if (windows && !info.device && !info.absolute) {
            let protect = drive(body);
            for (let i = 0; i < path.length && !protect; i++) {
                if (path[i] === ':' && (i + 1 === path.length || separator(path[i + 1]))) protect = true;
            }
            if (protect) body = '.\\' + body;
        }
        return info.device + (info.absolute ? sep : '') + body;
    }
    function isAbsolute(path) { string(path); return root(path).absolute; }
    function resolve(...paths) {
        for (let i = 0; i < paths.length; i++) string(paths[i]);
        let device = '', absolute = false, body = '';
        for (let i = paths.length - 1; i >= -1; i--) {
            let part = i >= 0 ? paths[i] : cwd();
            if (i === -1 && !windows && isWindows) {
                part = joinArray(split(part, '\\'), '/');
                if (drive(part)) part = slice(part, 2);
            }
            if (!part) continue;
            let info = root(part);
            if (i === -1 && windows && device && info.device && lower(info.device) !== lower(device)) {
                part = device + '\\'; info = root(part);
            }
            if (info.device) {
                if (device && lower(device) !== lower(info.device)) continue;
                if (!device) device = info.device;
            }
            if (!absolute) { body = slice(part, info.end) + sep + body; absolute = info.absolute; }
            if (absolute && (!windows || device)) break;
        }
        body = tail(body, absolute);
        return device + (absolute ? sep : '') + body || '.';
    }
    function join(...paths) {
        let combined = '', first = '';
        for (let i = 0; i < paths.length; i++) {
            const path = string(paths[i]);
            if (!path) continue;
            if (!combined) { combined = path; first = path; }
            else combined += sep + path;
        }
        if (!combined) return '.';
        if (windows && !(separator(first[0]) && separator(first[1]) && first.length > 2 && !separator(first[2]))) {
            let leading = 0;
            while (separator(combined[leading])) leading++;
            if (leading > 1) combined = '\\' + slice(combined, leading);
        }
        return normalize(combined);
    }
    function basename(path, suffix) {
        string(path);
        if (suffix !== undefined) string(suffix);
        if (suffix && suffix === path) return '';
        const start = windows && drive(path) ? 2 : 0;
        let end = path.length;
        while (end > start && separator(path[end - 1])) end--;
        let begin = end;
        while (begin > start && !separator(path[begin - 1])) begin--;
        let result = slice(path, begin, end);
        if (!result && suffix && suffix.length <= path.length) return path;
        if (suffix && suffix.length < result.length && slice(result, -suffix.length) === suffix) result = slice(result, 0, -suffix.length);
        return result;
    }
    function extension(base) {
        if (base === '..') return '';
        const index = lastIndex(base, '.');
        return index > 0 ? slice(base, index) : '';
    }
    function extname(path) { return extension(basename(path)); }
    function parse(path) {
        string(path);
        const info = root(path), prefix = slice(path, 0, info.end);
        let end = path.length;
        while (end > info.end && separator(path[end - 1])) end--;
        let start = end;
        while (start > info.end && !separator(path[start - 1])) start--;
        const base = slice(path, start, end);
        const ext = !windows && info.absolute && start === 1 && base === '..' ? '.' : extension(base);
        const dir = start > info.end ? slice(path, 0, start - 1) : prefix;
        return {root: prefix, dir, base, ext, name: slice(base, 0, base.length - ext.length)};
    }
    function dirname(path) {
        string(path);
        const info = root(path);
        let end = path.length;
        while (end > info.end && separator(path[end - 1])) end--;
        while (end > info.end && !separator(path[end - 1])) end--;
        if (end <= info.end) return slice(path, 0, info.end) || '.';
        if (!windows && end === 2 && path[0] === '/') return '//';
        return slice(path, 0, end - 1);
    }
    function format(object) {
        if (object === null || typeof object !== 'object' || isArray(object)) throw new TypeErrorCtor('path object must be an object');
        const values = create(null);
        for (const name of ['dir', 'root', 'base', 'name', 'ext']) {
            const value = object[name];
            if (value !== undefined && typeof value !== 'string') throw new TypeErrorCtor(`path.${name} must be a string`);
            values[name] = value || '';
        }
        const dir = values.dir || values.root;
        const ext = values.ext && values.ext[0] !== '.' ? '.' + values.ext : values.ext;
        const base = values.base || values.name + ext;
        return dir ? dir + (dir === values.root ? '' : sep) + base : base;
    }
    function relative(from, to) {
        from = resolve(string(from)); to = resolve(string(to));
        const a = components(from), b = components(to);
        let common = 0;
        while (common < a.length && common < b.length && (windows ? lower(a[common]) === lower(b[common]) : a[common] === b[common])) common++;
        if (windows && common === 0 && a.length && b.length) return to;
        const result = [];
        for (let i = common; i < a.length; i++) push(result, '..');
        for (let i = common; i < b.length; i++) push(result, b[i]);
        return joinArray(result, sep);
    }
    function toNamespacedPath(path) {
        if (!windows || typeof path !== 'string' || path.length === 0) return path;
        const resolved = resolve(path);
        if (resolved.length <= 2) return path;
        if (resolved[0] === '\\' && resolved[1] === '\\' && resolved[2] !== '?' && resolved[2] !== '.') return '\\\\?\\UNC\\' + slice(resolved, 2);
        if (drive(resolved) && resolved[2] === '\\') return '\\\\?\\' + resolved;
        return resolved;
    }
    return {resolve, normalize, isAbsolute, join, relative, toNamespacedPath, dirname, basename, extname, format, parse, sep, delimiter: windows ? ';' : ':'};
}

const posix = variant(false), win32 = variant(true);
posix.posix = win32.posix = posix;
posix.win32 = win32.win32 = win32;
return isWindows ? win32 : posix;
