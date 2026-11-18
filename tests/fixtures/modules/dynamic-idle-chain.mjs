const module = await import('./dynamic-idle.mjs');
if (module.value !== 42) throw Error('nested idle recovery');
console.log('recovered chain');
