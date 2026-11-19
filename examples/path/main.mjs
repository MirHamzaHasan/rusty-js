import { posix, win32 } from 'node:path';

console.log(posix.normalize('src/../app/main.js'));
console.log(win32.join('C:\\app', '..', 'data', 'rows.csv'));
const file = posix.parse('/data/rows.csv');
console.log(file.name, file.ext);
