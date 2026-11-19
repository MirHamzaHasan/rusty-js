import { Readable } from 'node:stream';

async function* rows() {
  try {
    yield 1;
    await Rusty.sleep(1);
    yield 2;
    yield 3;
  } finally {
    console.log('source released');
  }
}

const source = Readable.from(rows(), { highWaterMark: 1 });
for await (const row of source) {
  console.log('row:', row);
  if (row === 2) break;
}
console.log('closed:', source.closed);
