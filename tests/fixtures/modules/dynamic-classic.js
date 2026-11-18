import('./dynamic-value.mjs').then(module => {
  if (module.main !== false || module.value !== 1) throw Error('classic import');
  console.log('classic dynamic', module.value);
});
