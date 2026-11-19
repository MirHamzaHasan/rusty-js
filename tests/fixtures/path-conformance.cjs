// Deterministic lexical correctness corpus, run unchanged with Node and RustyJS.
const path = require('node:path');
const paths = ['', '.', '..', '...', '/', '//', '///', '/foo', '//foo', '///foo', '/foo/',
  'a/b', 'a//b/', 'a/../../b', './x/../', '.env', '.env.local', 'file.', 'dir/.ext',
  'a/../C:/x', 'C:', 'C:foo', 'C:/foo/../bar', 'c:\\foo\\bar', 'D:relative',
  '\\rooted', '\\\\server\\share', '//server/share/', '//server/share/..',
  '//server', '//s//share///x', '\\\\?\\C:\\x\\..\\y', '\\\\.\\pipe\\name',
  'space # café/💩.js', 'foo:bar', 'a\\b', 'C:/', 'C:./', 'dir/file.txt', '/..'];
const parts = ['', '.', '..', '...', 'a', 'B', 'c.txt', '.env', 'file.', 'é', '💩', 'C:', 's:thing'];
let seed = 712367;
const random = n => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed % n; };
for (let n = 0; n < 400; n++) {
  let value = ['', '/', '//', 'C:', 'C:/', '\\', '\\\\server\\share\\'][random(7)];
  const count = 1 + random(5);
  for (let i = 0; i < count; i++) value += (i ? ['/', '//', '\\'][random(3)] : '') + parts[random(parts.length)];
  if (random(3) === 0) value += ['/', '\\'][random(2)];
  paths.push(value);
}
const results = [];
for (const name of ['posix', 'win32']) {
  const p = path[name];
  for (let i = 0; i < paths.length; i++) {
    const value = paths[i], other = paths[(i * 17 + 11) % paths.length], parsed = p.parse(value);
    results.push([name, 'path', value, p.normalize(value), p.dirname(value), p.basename(value), p.extname(value), parsed,
      p.isAbsolute(value), p.toNamespacedPath(value), p.format(parsed), p.basename(value, '.txt'), p.basename(value, p.basename(value))]);
    results.push([name, 'pair', value, other, p.resolve(value, other), p.join(value, other), p.relative(value, other)]);
  }
}
console.log(JSON.stringify(results));
