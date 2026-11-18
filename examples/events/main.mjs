import { EventEmitter } from 'node:events';
import { sleep } from 'rustyjs:timers';

const bus = new EventEmitter();
bus.once('ready', function(value) {
  if (this !== bus) throw Error('listener receiver changed');
  console.log('event:', value);
});
await sleep(1);
bus.emit('ready', 42);
bus.emit('ready', 84);
