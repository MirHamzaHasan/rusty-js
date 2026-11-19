import { Readable, Writable } from 'node:stream';
import { pipeline } from 'node:stream/promises';

const source = Readable.from(['hello ', 'world'], { objectMode: false });
const chunks = [];
const sink = new Writable({
  highWaterMark: 4,
  write(chunk, encoding, done) {
    setTimeout(() => { chunks.push(chunk); done(); }, 1);
  },
});

await pipeline(source, sink);
console.log(Buffer.concat(chunks).toString());
console.log('closed:', source.closed, sink.closed);
