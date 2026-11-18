// Run unchanged with Node and RustyJS; compare stdout. This is correctness, not performance.
const results = [];
for (let a = 0; a < 256; a++) {
    const bytes = Buffer.from([a, (a * 17) & 255, (255 - a), 0]);
    for (const enc of ['hex', 'base64', 'base64url', 'latin1', 'ascii', 'utf16le', 'utf8']) {
        results.push([enc, bytes.toString(enc), Buffer.from(bytes.toString(enc), enc).toString('hex')]);
    }
    const b = Buffer.alloc(8);
    b.writeUInt32BE(a * 65537, 1);
    results.push([b.toString('hex'), b.readUInt32LE(1), b.readInt16BE(2)]);
}
for (const text of ['', 'ASCII', 'é漢字', '💩', '\ud800', '\udc00', 'a\ud800b', '\ud800\udc00', '\u0000']) {
    for (const enc of ['utf8', 'utf16le', 'latin1', 'ascii', 'hex', 'base64', 'base64url']) {
        results.push([text, enc, Buffer.from(text, enc).toString('hex')]);
        for (let length = 0; length < 10; length++) {
            const buffer = Buffer.alloc(length, 46);
            results.push([buffer.write(text, 0, length, enc), buffer.toString('hex')]);
        }
    }
}
for (const text of ['Z', 'Zg', 'Zg=', 'Zg===junk', 'a-b_c+d/e', ' Z\n m9v! ', '=Zg', 'Z=g', 'Zg=Zg', '000x', 'aabbc', 'ffgg']) {
    for (const enc of ['hex', 'base64', 'base64url']) results.push([text, enc, Buffer.from(text, enc).toString('hex')]);
}
console.log(JSON.stringify(results));
