import process from 'node:process';
import { platform, arch, endianness } from 'node:os';

console.log('runtime:', process.release.name, process.version);
console.log('host:', platform(), arch(), endianness());
console.log('args:', process.argv.slice(2).join(','));
await Rusty.sleep(0);
console.log('async work drained');
