import * as publicValues from 'rustyjs-example-package/values/answer';
import * as privateValues from '#answer';

const dynamicValues = await import('rustyjs-example-package/values/answer');
if (publicValues !== privateValues || privateValues !== dynamicValues) {
  throw Error('package maps lost namespace identity');
}
console.log('package maps:', publicValues.answer);
