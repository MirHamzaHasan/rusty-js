// Selected values and cleanup outcomes; not a claim about event/tick parity.
const { Readable, Writable } = require('node:stream');
const { pipeline, finished } = require('node:stream/promises');

async function main() {
  const results = [];
  for (let n = 0; n < 64; n++) {
    const values = [];
    for await (const value of Readable.from([n, 0, undefined, false], { highWaterMark: 1 })) {
      values.push(value === undefined ? 'undefined' : value);
    }
    results.push(values);

    const chunks = [];
    const sink = new Writable({ highWaterMark: 1, write(chunk, encoding, done) {
      chunks.push(chunk);
      Promise.resolve().then(() => done());
    }});
    await pipeline(Readable.from([String(n), ':end'], { objectMode: false }), sink);
    results.push([Buffer.concat(chunks).toString(), sink.writableFinished, sink.closed]);

    let closed = 0;
    async function* source() { try { yield n; yield n + 1; } finally { closed++; } }
    const early = Readable.from(source(), { highWaterMark: 1 });
    for await (const value of early) { results.push([value]); break; }
    results.push([closed, early.destroyed]);

    const preserved = Readable.from([n, n + 1, n + 2], { highWaterMark: 1 });
    for await (const value of preserved.iterator({ destroyOnReturn: false })) break;
    const rest = [];
    for await (const value of preserved) rest.push(value);
    results.push(rest);

    const retained = new Writable({ autoDestroy: false, write(chunk, encoding, done) { done(); } });
    retained.end();
    await finished(retained, { cleanup: true });
    results.push([retained.writableFinished, retained.destroyed]);
  }
  console.log(JSON.stringify(results));
}
main().catch(error => { throw error; });
