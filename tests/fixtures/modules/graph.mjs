import { shared as left } from "./left.mjs";
import { shared as right, value } from "./nested/right.mjs";
import shared from "./nested/../shared.mjs";

if (left !== right || left !== shared || value !== 2 || moduleRuns !== 1) {
  throw Error("module identity or live bindings were lost");
}
console.log("graph", moduleRuns, value);
