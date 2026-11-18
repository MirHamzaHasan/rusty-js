globalThis.failureRuns = (globalThis.failureRuns ?? 0) + 1;
globalThis.failure = { message: 'original rejection' };
await Rusty.sleep(1);
throw globalThis.failure;
