const originalSleep = Rusty.sleep;
const originalTimeout = setTimeout;
globalThis.Rusty = null;
for (const name of ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval']) {
  Object.defineProperty(globalThis, name, { get() { throw Error('read patched global'); } });
}
import('rustyjs:timers').then(async timers => {
  if (timers.sleep !== originalSleep || timers.setTimeout !== originalTimeout) throw Error('lost native identity');
  await timers.sleep(0);
  await new Promise(resolve => timers.setTimeout(resolve, 0));
  console.log('captured native timers');
});
