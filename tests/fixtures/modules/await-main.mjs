import { answer } from "./awaited.mjs";
await Promise.resolve();
await Rusty.sleep(0);
console.log("awaited", answer);
