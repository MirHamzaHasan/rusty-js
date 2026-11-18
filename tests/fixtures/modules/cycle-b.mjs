import { nameA } from "./cycle-a.mjs";
export function nameB() { return "B"; }
export function names() { return nameA() + nameB(); }
