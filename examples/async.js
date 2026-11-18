console.log("start");
Rusty.sleep(10).then(() => console.log("Tokio task finished"));
console.log("script finished; the host keeps running until idle");
