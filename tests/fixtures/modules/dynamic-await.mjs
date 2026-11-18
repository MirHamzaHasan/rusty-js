globalThis.dynamicRuns = (globalThis.dynamicRuns ?? 0) + 1;
await Rusty.sleep(1);
await new Promise(resolve => setTimeout(resolve, 1));
export const value = (await import('./space%20%23%20caf%C3%A9.mjs')).value;
