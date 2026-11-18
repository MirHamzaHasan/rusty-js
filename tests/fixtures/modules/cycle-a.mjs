import { nameB } from "./cycle-b.mjs";
export function nameA() { return "A"; }
export function names() { return nameA() + nameB(); }
