import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

export function resolveBase(input: string | undefined, marker: string): string {
  const p = resolve(input ?? ".");
  if (existsSync(join(p, marker))) return p;
  if (existsSync(join(p, "utils", "quint", "pipeline", marker))) {
    return join(p, "utils", "quint", "pipeline");
  }
  return p;
}

export function loadJson(path: string): any {
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch (e: any) {
    console.error(`${path}: ${e?.message ?? e}`);
    process.exit(1);
  }
}

export function fail(msg: string): void {
  console.error(`FAIL: ${msg}`);
}

export const EXIT_OK = 0;
export const EXIT_FAIL = 1;
export const EXIT_USAGE = 2;
