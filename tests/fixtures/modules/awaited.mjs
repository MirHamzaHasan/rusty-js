await Rusty.sleep(0);
export const answer = await new Promise(resolve => setTimeout(resolve, 0, 42));
