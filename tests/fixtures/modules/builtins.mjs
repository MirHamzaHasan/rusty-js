import * as timers from 'rustyjs:timers';
import { sleep, setTimeout as timeout, clearTimeout as clear, setInterval as interval, clearInterval } from 'rustyjs:timers';
import { sleep as reexported } from './builtin-reexport.mjs';
const assert = (value, message) => { if (!value) throw Error(message); };
const [a, b] = await Promise.all([import('rustyjs:timers'), import('rustyjs:timers')]);
assert(a === b && a === timers, 'one cached native namespace');
assert(sleep === Rusty.sleep && sleep === reexported, 'original sleep identity');
assert(timeout === globalThis.setTimeout && clear === globalThis.clearTimeout, 'native timer identity');
assert(interval === globalThis.setInterval && clearInterval === globalThis.clearInterval, 'native interval identity');
assert(Object.keys(timers).sort().join(',') === 'clearInterval,clearTimeout,setInterval,setTimeout,sleep', 'named exports only');
let count = 0;
const cancelled = timeout(() => { throw Error('cancelled timer fired'); }, 2147483647);
clearInterval(cancelled);
await sleep(0);
await new Promise(resolve => timeout(resolve, 0));
await new Promise(resolve => {
  const id = interval(() => {
    if (++count === 2) { clear(id); resolve(); }
  }, 0);
});
try { sleep('1'); throw Error('coerced sleep'); } catch (error) { assert(error instanceof TypeError, 'sleep validation'); }
try { timeout('code'); throw Error('evaluated timer string'); } catch (error) { assert(error instanceof TypeError, 'timer validation'); }
console.log('built-in timers', count);
