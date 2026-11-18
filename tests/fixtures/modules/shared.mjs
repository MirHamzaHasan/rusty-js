globalThis.moduleRuns = (globalThis.moduleRuns ?? 0) + 1;
export let value = 1;
export function increment() { ++value; }
export default { shared: true };
