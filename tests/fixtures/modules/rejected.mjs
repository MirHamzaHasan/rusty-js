setInterval(() => {}, 2147483647);
await Rusty.sleep(0);
throw Error("module-failed");
