const counter = require('./counter');
const { sleep } = require('rustyjs:timers');

console.log('CommonJS:', counter.next());
sleep(1).then(async () => {
  const imported = await import('./counter.js');
  if (imported.default !== counter) throw Error('module identity changed');
  console.log('shared counter:', imported.default.next());
});
