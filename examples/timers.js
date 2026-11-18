const cancelled = setTimeout(() => console.log("this must not print"), 1000);
clearTimeout(cancelled);

let ticks = 0;
const interval = setInterval((label) => {
  console.log(label, ++ticks);
  if (ticks === 3) clearInterval(interval);
}, 5, "tick");

setTimeout((answer) => console.log("timeout", answer), 0, 42);
