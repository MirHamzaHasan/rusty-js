// Compare stdout with Node and RustyJS. Selected values/pressure, not tick-phase parity.
const { Readable, Writable } = require('node:stream');

async function main() {
  const results = [];
  for (let n = 0; n < 128; n++) {
    const source = new Readable({ highWaterMark: 4, read() {} });
    const input = Buffer.from([n, (n * 7) & 255, (255 - n), 0, 42, 99]);
    const pressure = [source.push(input.subarray(0, 2)), source.push(input.subarray(2))];
    const first = source.read(1);
    const second = source.read(3);
    source.push(null);
    const last = source.read(8);
    results.push([pressure, first.toString('hex'), second.toString('hex'), last.toString('hex')]);
    source.resume();

    const chunks = [];
    const piped = Readable.from([input.subarray(0, 2), input.subarray(2)], { objectMode: false, highWaterMark: 2 });
    const sink = new Writable({ highWaterMark: 1, write(chunk, encoding, done) {
      chunks.push(chunk.toString('hex'));
      Promise.resolve().then(() => done());
    }});
    await new Promise((resolve, reject) => {
      sink.on('error', reject);
      piped.on('error', reject);
      sink.on('close', resolve);
      piped.pipe(sink);
    });
    results.push([chunks, sink.writableFinished, sink.closed, piped.readableEnded]);
  }
  console.log(JSON.stringify(results));
}

main().catch(error => { throw error; });
