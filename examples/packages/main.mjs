import * as values from 'rustyjs-example-package';
import { doubled } from 'rustyjs-example-package/double';

const again = await import('rustyjs-example-package');
if (values !== again) throw Error('package imports lost namespace identity');
console.log('package imports:', values.answer, doubled);
