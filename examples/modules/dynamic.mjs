const specifier = './math.mjs';
const math = await import(specifier);
console.log('dynamic answer:', math.answer);
console.log('entrypoint:', import.meta.main);
console.log('module URL:', import.meta.url);
