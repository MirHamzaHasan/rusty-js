import { Buffer } from 'node:buffer';

const packet = Buffer.alloc(8);
packet.writeUInt32BE(42, 0);
packet.write('OK', 4);
const payload = packet.subarray(4, 6);
console.log(packet.readUInt32BE(0), payload.toString());

const copy = Buffer.from(payload);
payload[0] = 78;
console.log('shared:', packet.toString('ascii', 4, 6), 'copy:', copy.toString());
