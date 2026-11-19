import { Readable, Writable } from 'node:stream';

const source = Readable.from(['hello ', 'world'], { objectMode: false });
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
  const fail = error => {
    source.destroy();
    sink.destroy();
    reject(error);
  };
  source.on('error', fail);
  sink.on('error', fail);
  sink.on('finish', resolve);
  source.pipe(sink);
});
console.log(chunks.join(''));
