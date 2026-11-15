import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { resolveBase, loadJson, fail } from "../lib/common.ts";
import { listQntFiles } from "../lib/qnt.ts";

const EXEMPT_REASONS = new Set(["environment", "no-backing", "observer"]);

interface Tag {
  kind: string;
  id: string;
  line: number;
}
interface Def {
  file: string;
  kind: string;
  name: string;
  line: number;
  tags: Tag[];
}

export function run(args: string[]): number {
  const generate = args.includes("--generate");
  const positional = args.filter((a) => !a.startsWith("--"));
  const specs = resolveBase(positional[0], "events.json");

  let failed = false;
  const warnings: string[] = [];
  const failWrap = (msg: string) => {
    fail(msg);
    failed = true;
  };
  const warn = (msg: string) => {
    warnings.push(msg);
    console.error(`WARN: ${msg}`);
  };

  function read(p: string): string {
    try {
      return readFileSync(p, "utf8");
    } catch {
      failWrap(`cannot read ${p}`);
      return "";
    }
  }

  const assumptionsJson: any = (() => {
    try {
      return JSON.parse(read(join(specs, "assumptions.json")));
    } catch (e: any) {
      failWrap(`cannot parse assumptions.json: ${e?.message ?? e}`);
      return { assumptions: [] };
    }
  })();
  const scopeJson: any = (() => {
    try {
      return JSON.parse(read(join(specs, "scope.json")));
    } catch (e: any) {
      failWrap(`cannot parse scope.json: ${e?.message ?? e}`);
      return { scopes: [] };
    }
  })();
  const requirementsJson: any = (() => {
    try {
      return JSON.parse(read(join(specs, "requirements.json")));
    } catch (e: any) {
      failWrap(`cannot parse requirements.json: ${e?.message ?? e}`);
      return { requirements: [] };
    }
  })();

  const assIds = new Set<string>(
    (assumptionsJson.assumptions ?? []).map((a: any) => a.id),
  );
  const reqIds = new Set<string>(
    (requirementsJson.requirements ?? []).map((r: any) => r.id),
  );

  const scopeIds = new Map<string, { title: string }>();
  for (const s of scopeJson.scopes ?? []) {
    for (const field of ["bound", "why_safe", "lifting"]) {
      if (typeof s[field] !== "string" || s[field].trim() === "") {
        failWrap(`scope.json: ${s.id} has no non-empty \`${field}\` field`);
      }
    }
    scopeIds.set(s.id, { title: s.title });
  }

  const qntFiles = existsSync(specs)
    ? readdirSync(specs)
        .filter((f) => f.endsWith(".qnt"))
        .sort()
    : [];
  if (qntFiles.length === 0) failWrap(`no .qnt files in ${specs}`);

  const defs: Def[] = [];
  for (const f of qntFiles) {
    const lines = read(join(specs, f)).split("\n");
    for (let i = 0; i < lines.length; i++) {
      const m = lines[i].match(
        /^\s*(?:pure\s+)?(action|val|def|run)\s+([A-Za-z_]\w*)/,
      );
      if (!m) continue;
      const tags: Tag[] = [];
      for (let j = i - 1; j >= 0 && /^\s*\/\//.test(lines[j]); j--) {
        const line = lines[j];
        for (const tm of line.matchAll(/@assumes\s+(ASS-\d+)\b/g))
          tags.push({ kind: "assumes", id: tm[1], line: j + 1 });
        for (const tm of line.matchAll(/@requires\s+(REQ-\d+)\b/g))
          tags.push({ kind: "requires", id: tm[1], line: j + 1 });
        for (const tm of line.matchAll(/@scope\s+(SCOPE-\d+)\b/g))
          tags.push({ kind: "scope", id: tm[1], line: j + 1 });
        for (const tm of line.matchAll(/@exempt\s+([a-z-]+)/g))
          tags.push({ kind: "exempt", id: tm[1], line: j + 1 });
      }
      defs.push({
        file: f,
        kind: m[1] === "def" ? "pure def" : m[1],
        name: m[2],
        line: i + 1,
        tags,
      });
    }
  }

  const citedAss = new Map<string, string[]>();
  const citedScope = new Map<string, string[]>();
  function cite(map: Map<string, string[]>, id: string, where: string): void {
    if (!map.has(id)) map.set(id, []);
    map.get(id)!.push(where);
  }

  for (const d of defs) {
    const where = `${d.file}:${d.name}`;
    if (d.tags.length === 0) {
      failWrap(
        `${where}:${d.line}: ${d.kind} has no @assumes/@requires/@scope/@exempt tag`,
      );
      continue;
    }
    for (const t of d.tags) {
      if (t.kind === "assumes") {
        if (!assIds.has(t.id))
          failWrap(`${where}:${t.line}: unknown assumption ${t.id}`);
        else cite(citedAss, t.id, where);
      } else if (t.kind === "requires") {
        if (!reqIds.has(t.id))
          failWrap(`${where}:${t.line}: unknown requirement ${t.id}`);
      } else if (t.kind === "scope") {
        if (!scopeIds.has(t.id))
          failWrap(`${where}:${t.line}: unknown scope ${t.id}`);
        else cite(citedScope, t.id, where);
      } else if (t.kind === "exempt") {
        if (!EXEMPT_REASONS.has(t.id))
          failWrap(
            `${where}:${t.line}: unknown @exempt reason '${t.id}' (expected one of ${[...EXEMPT_REASONS].join(", ")})`,
          );
      }
    }
  }

  for (const id of assIds) {
    if (!citedAss.has(id))
      failWrap(
        `unreferenced assumption ${id}: no .qnt definition carries @assumes ${id}`,
      );
  }
  for (const [id] of scopeIds) {
    if (!citedScope.has(id))
      failWrap(
        `unreferenced scope ${id}: no .qnt definition carries @scope ${id}`,
      );
  }
  const noBacking = defs.filter((d) =>
    d.tags.some((t) => t.kind === "exempt" && t.id === "no-backing"),
  );
  if (noBacking.length > 0)
    warn(
      `no-backing exemptions: ${noBacking.map((d) => `${d.file}:${d.name}`).join(", ")}`,
    );

  // generated traceability
  const rows: any[] = [];
  function pushRow(
    id: string,
    kind: string,
    title: string,
    refs: string[],
  ): void {
    rows.push({ id, kind, title, refs: refs.slice().sort() });
  }
  for (const a of assumptionsJson.assumptions ?? []) {
    pushRow(a.id, "ASS", a.title, citedAss.get(a.id) ?? []);
  }
  for (const s of scopeJson.scopes ?? []) {
    pushRow(s.id, "SCOPE", s.title, citedScope.get(s.id) ?? []);
  }

  const generatedTrace = JSON.stringify({ version: 1, rows }, null, 2) + "\n";
  const tracePath = join(specs, "traceability.json");
  let traceText = "";
  try {
    traceText = readFileSync(tracePath, "utf8");
  } catch {
    if (!generate) failWrap(`cannot read ${tracePath}`);
  }

  if (generate) {
    if (traceText !== generatedTrace) {
      writeFileSync(tracePath, generatedTrace);
      console.error(
        "specs-check: regenerated utils/quint/pipeline/traceability.json",
      );
    }
  } else if (traceText !== generatedTrace) {
    failWrap(
      "utils/quint/pipeline/traceability.json is stale (run `node utils/quint/coverage/bin/check.ts specs --generate` after reviewing the diff)",
    );
  }

  if (!failed) {
    const nTag = defs.reduce(
      (a, d) => a + d.tags.filter((t) => t.kind !== "exempt").length,
      0,
    );
    const nEx = defs.reduce(
      (a, d) => a + d.tags.filter((t) => t.kind === "exempt").length,
      0,
    );
    console.error(
      `specs-check: OK (${defs.length} definitions, ${nTag} tags, ${nEx} exemptions)`,
    );
    if (warnings.length > 0)
      console.error(`specs-check: ${warnings.length} warning(s)`);
  }
  return failed ? 1 : 0;
}
