import { sleep, setTimeout, clearTimeout } from 'rustyjs:timers';

const cancelled = setTimeout(() => console.log('cancelled callback'), 1000);
clearTimeout(cancelled);
await sleep(5);
await new Promise(resolve => setTimeout(resolve, 0));
console.log('native module timers completed');
