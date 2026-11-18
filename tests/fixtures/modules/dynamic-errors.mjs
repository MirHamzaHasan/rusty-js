const assert = (value, message) => { if (!value) throw Error(message); };
for (const specifier of ['./does-not-exist.mjs', 'node:fs', 'package', './dynamic-value.mjs?query', './dynamic-value.mjs#hash', './nested%2fdependency.mjs']) {
  try { await import(specifier); throw Error('accepted invalid specifier'); }
  catch (error) { assert(error instanceof TypeError, `expected TypeError: ${error}`); }
}
try { await import('./syntax.mjs'); throw Error('accepted syntax error'); }
catch (error) { assert(error instanceof SyntaxError, 'preserve syntax error type'); }
try { await import('./missing-export.mjs'); throw Error('accepted missing export'); }
catch (error) { assert(error instanceof SyntaxError, 'preserve link error type'); }
try { await import('./dynamic-value.mjs', { with: { type: 'json' } }); throw Error('accepted attributes'); }
catch (error) { assert(error instanceof TypeError, 'attributes reject'); }
let previous;
for (let i = 0; i < 2; i++) {
  try { await import('./dynamic-rejected.mjs'); throw Error('accepted failed module'); }
  catch (error) {
    assert(error === globalThis.failure, 'preserve thrown value identity');
    assert(i === 0 || error === previous, 'cached evaluation failure');
    previous = error;
  }
}
assert(globalThis.failureRuns === 1, 'failed evaluation runs once');
console.log('caught dynamic failures');
