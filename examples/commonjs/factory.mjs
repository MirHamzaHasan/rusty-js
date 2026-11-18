import { createRequire } from 'rustyjs:module';
import counter from './counter.js';

const require = createRequire(import.meta.url);
if (require('./counter') !== counter) throw Error('factory changed module identity');
console.log('created require:', require('./counter').next());
