for (const specifier of ['rustyjs:unknown', 'rustyjs:timers?query', 'rustyjs:timers#hash', 'rustyjs:timers/child', 'rustyjs:TIMERS']) {
  try { await import(specifier); throw Error('accepted unknown built-in'); }
  catch (error) { if (!(error instanceof TypeError) || !error.message.includes('unknown built-in module')) throw error; }
}
try { await import('rustyjs:timers', { with: { type: 'json' } }); throw Error('accepted attributes'); }
catch (error) { if (!(error instanceof TypeError)) throw error; }
try { await import('./builtin-missing-export.mjs'); throw Error('accepted missing export'); }
catch (error) { if (!(error instanceof SyntaxError)) throw error; }
const timers = await import('rustyjs:timers');
await timers.sleep(0);
console.log('caught built-in failures');
