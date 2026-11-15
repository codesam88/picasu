import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";

export interface Declaration {
  file: string;
  kind: string; // action | val | def | run | 'pure def'
  name: string;
  line: number; // 1-based
}

export function listQntFiles(dir: string): string[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((f) => f.endsWith(".qnt"))
    .sort();
}

export function scanDeclarations(dir: string): Declaration[] {
  const out: Declaration[] = [];
  for (const f of listQntFiles(dir)) {
    const text = readFileSync(join(dir, f), "utf8");
    const lines = text.split("\n");
    for (let i = 0; i < lines.length; i++) {
      const m = lines[i].match(
        /^\s*(?:pure\s+)?(action|val|def|run)\s+([A-Za-z_]\w*)/,
      );
      if (!m) continue;
      const kind = m[1] === "def" ? "pure def" : m[1];
      out.push({ file: f, kind, name: m[2], line: i + 1 });
    }
  }
  return out;
}

export function declarationNames(dir: string): Set<string> {
  return new Set(scanDeclarations(dir).map((d) => d.name));
}

// Collect @operation tags across .qnt files.
export function collectOperationTags(dir: string): Set<string> {
  const tags = new Set<string>();
  for (const f of listQntFiles(dir)) {
    const text = readFileSync(join(dir, f), "utf8");
    const re = /@operation\s+(OP-\d+)/g;
    let m: RegExpExecArray | null;
    while ((m = re.exec(text)) !== null) tags.add(m[1]);
  }
  return tags;
}
