try {
  await import('./pending.mjs');
  throw Error('accepted idle module');
} catch (error) {
  if (!(error instanceof TypeError) || !error.message.includes('pending with no active host work')) throw error;
}
console.log('caught idle import');
export const value = 42;
