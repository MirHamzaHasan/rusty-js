const EventEmitter = require('events');
const bus = new EventEmitter();

bus.on('value', value => console.log('CommonJS event:', value));
bus.emit('value', 42);
