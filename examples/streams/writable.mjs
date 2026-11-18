import { Writable } from 'node:stream';

const chunks = [];
const sink = new Writable({
  highWaterMark: 4,
  write(chunk, encoding, done) {
    setTimeout(() => {
      chunks.push(chunk.toString());
      done();
    }, 1);
  },
});

await new Promise((resolve, reject) => {
  sink.on('error', reject);
  sink.on('finish', resolve);
  if (sink.write('hello ')) sink.end('world');
  else sink.once('drain', () => sink.end('world'));
});
console.log(chunks.join(''));
