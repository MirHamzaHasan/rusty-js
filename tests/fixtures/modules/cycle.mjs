import { names as namesA } from "./cycle-a.mjs";
import { names as namesB } from "./cycle-b.mjs";
console.log(namesA(), namesB());
