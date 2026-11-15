import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve, relative, dirname } from "node:path";
import { resolveBase, loadJson, fail } from "../lib/common.ts";
import { declarationNames, collectOperationTags } from "../lib/qnt.ts";

const ASPECTS = new Set(["asset_file", "sidecar", "thumbnail", "db_row"]);
const STATUSES = new Set(["guarded", "open", "deferred"]);
const NODES = new Set(Array.from({ length: 13 }, (_, i) => `N${i + 1}`));
const EXPECTED_OPS = new Set(
  Array.from({ length: 12 }, (_, i) => `OP-${i + 1}`),
);

const EVENT_OWNER_EXEMPT = new Map<string, string>([
  [
    "API_READ_COMPRESSED",
    "read-only boundary event: serves compressed bytes; no mutation operation tracked",
  ],
  [
    "API_READ_ORIGINAL",
    "read-only boundary event: serves original bytes; no mutation operation tracked",
  ],
  [
    "API_READ_MISSING",
    "read-only boundary event: missing-asset response path; no mutation operation tracked",
  ],
  [
    "API_READ_MOVED",
    "read-only boundary event: moved-asset response path; no mutation operation tracked",
  ],
  [
    "PROCESS_CRASH",
    "recovery event: crash recovery is handled by the pipeline generally",
  ],
  [
    "DERIVED_STORE_LOSS",
    "recovery event: derived-store loss recovery is handled by the pipeline generally",
  ],
  [
    "DB_LOSS",
    "recovery event: DB loss recovery is handled by the pipeline generally",
  ],
]);

export function run(args: string[]): number {
  const base = resolveBase(args[0], "operations.json");
  const opsPath = join(base, "operations.json");
  const eventsPath = join(base, "events.json");
  const repoRoot = base.endsWith("pipeline")
    ? resolve(base, "..", "..", "..")
    : base;
  // Scenario-yaml refs live under <source-dir>/../tests/scenarios; the
  // source dir defaults to <root>/backend/src and can be overridden.
  const scenariosRel = relative(
    repoRoot,
    join(
      dirname(resolve(args[1] ? args[1] : join(repoRoot, "backend", "src"))),
      "tests",
      "scenarios",
    ),
  );

  let failed = false;
  const failWrap = (msg: string) => {
    fail(msg);
    failed = true;
  };
  const warn = (msg: string) => {
    console.warn(`WARN: ${msg}`);
  };

  function load(path: string): any {
    return loadJson(path);
  }

  const opsDoc = load(opsPath);
  const eventsDoc = existsSync(eventsPath) ? load(eventsPath) : null;

  if (typeof opsDoc.version !== "number")
    failWrap("operations.json: missing/invalid version");
  if (
    typeof opsDoc.description !== "string" ||
    opsDoc.description.trim() === ""
  ) {
    failWrap("operations.json: missing/empty top-level description");
  }

  const eventNames = new Set<string>(
    Array.isArray(eventsDoc?.events)
      ? eventsDoc.events.map((e: any) => e.name)
      : [],
  );

  const entries: any[] = Array.isArray(opsDoc.operations)
    ? opsDoc.operations
    : [];
  if (!Array.isArray(opsDoc.operations))
    failWrap("operations.json: operations must be an array");

  const modelDecls = existsSync(base)
    ? declarationNames(base)
    : new Set<string>();

  function validateTestRef(
    ref: string,
    label: string,
    scenariosRel: string,
  ): void {
    if (ref.endsWith(".yaml")) {
      const relPath = ref;
      if (relPath.startsWith("/") || relPath.split("/").includes("..")) {
        failWrap(
          `${label}: test ref path '${relPath}' must be repo-relative without '..'`,
        );
        return;
      }
      if (
        !relPath.startsWith(scenariosRel ? scenariosRel + "/" : scenariosRel)
      ) {
        failWrap(
          `${label}: yaml test ref '${relPath}' must live under backend/tests/scenarios/`,
        );
        return;
      }
      const full = join(repoRoot, relPath);
      if (!existsSync(full)) {
        failWrap(`${label}: test ref file '${relPath}' does not exist`);
      }
      return;
    }
    const sep = ref.lastIndexOf("::");
    if (sep <= 0 || sep + 2 >= ref.length) {
      failWrap(`${label}: test ref '${ref}' must be '<path>::<test_fn>'`);
      return;
    }
    const relPath = ref.slice(0, sep);
    const fnName = ref.slice(sep + 2);
    if (relPath.startsWith("/") || relPath.split("/").includes("..")) {
      failWrap(
        `${label}: test ref path '${relPath}' must be repo-relative without '..'`,
      );
      return;
    }
    if (!relPath.endsWith(".rs")) {
      failWrap(`${label}: test ref '${ref}' must reference a .rs file`);
      return;
    }
    if (!/^[A-Za-z_]\w*$/.test(fnName)) {
      failWrap(`${label}: test ref '${ref}' has an invalid test function name`);
      return;
    }
    const full = join(repoRoot, relPath);
    if (!existsSync(full)) {
      failWrap(`${label}: test ref file '${relPath}' does not exist`);
      return;
    }
    let text = "";
    try {
      text = readFileSync(full, "utf8");
    } catch (e: any) {
      failWrap(
        `${label}: cannot read test ref file '${relPath}': ${e?.message ?? e}`,
      );
      return;
    }
    if (!new RegExp(`\\bfn\\s+${fnName}\\b`).test(text)) {
      failWrap(`${label}: '${relPath}' does not define fn ${fnName}`);
    }
  }

  const seenIds = new Set<string>();
  const coverageRows: { op: string; model: string; test?: string }[] = [];

  for (let i = 0; i < entries.length; i++) {
    const op = entries[i];
    const label = `operations[${i}]${op && op.id ? ` (${op.id})` : ""}`;
    if (op == null || typeof op !== "object") {
      failWrap(`${label}: not an object`);
      continue;
    }

    for (const f of [
      "id",
      "name",
      "events",
      "reads",
      "writes",
      "guards",
      "status",
      "nodes",
      "notes",
    ]) {
      if (!(f in op)) failWrap(`${label}: missing required field '${f}'`);
    }

    if (!EXPECTED_OPS.has(op.id))
      failWrap(`${label}: id '${op.id}' not in OP-1..OP-12`);
    if (seenIds.has(op.id)) failWrap(`${label}: duplicate id '${op.id}'`);
    seenIds.add(op.id);

    if (typeof op.name !== "string" || op.name.trim() === "")
      failWrap(`${label}: name missing/empty`);

    if (!Array.isArray(op.events))
      failWrap(`${label}: events must be an array`);
    else
      for (const e of op.events) {
        if (typeof e !== "string" || e.trim() === "")
          failWrap(`${label}: event entry must be non-empty string`);
        else if (eventNames.size > 0 && !eventNames.has(e))
          failWrap(`${label}: event '${e}' not in events.json`);
      }

    for (const field of ["reads", "writes"]) {
      if (!Array.isArray(op[field]))
        failWrap(`${label}: ${field} must be an array`);
      else
        for (const a of op[field]) {
          if (!ASPECTS.has(a))
            failWrap(`${label}: ${field} item '${a}' not in aspect set`);
        }
    }

    if (!Array.isArray(op.guards))
      failWrap(`${label}: guards must be an array`);
    else
      for (const g of op.guards) {
        if (typeof g !== "string") failWrap(`${label}: guard must be string`);
      }

    if (op.properties !== undefined) {
      if (!Array.isArray(op.properties))
        failWrap(`${label}: properties must be an array`);
      else
        for (const p of op.properties) {
          if (p === null || typeof p !== "object" || Array.isArray(p)) {
            failWrap(
              `${label}: property entry must be an object {model, test?}`,
            );
            continue;
          }
          for (const key of Object.keys(p)) {
            if (key !== "model" && key !== "test") {
              failWrap(`${label}: property entry has unknown key '${key}'`);
            }
          }
          if (typeof p.model !== "string" || p.model.trim() === "") {
            failWrap(`${label}: property 'model' must be a non-empty string`);
          } else if (!modelDecls.has(p.model)) {
            failWrap(
              `operation '${op.id}' references unknown model declaration '${p.model}'`,
            );
          }
          let testRef: string | undefined;
          if (p.test !== undefined) {
            if (typeof p.test !== "string" || p.test.trim() === "") {
              failWrap(
                `${label}: property '${p.model}' test ref must be a non-empty string`,
              );
            } else {
              testRef = p.test;
              validateTestRef(
                p.test,
                `operation '${op.id}' property '${p.model}'`,
                scenariosRel,
              );
            }
          }
          coverageRows.push({ op: op.id, model: p.model, test: testRef });
        }
    }

    if (!STATUSES.has(op.status))
      failWrap(`${label}: status '${op.status}' not in guarded/open/deferred`);

    if (!Array.isArray(op.nodes)) failWrap(`${label}: nodes must be an array`);
    else
      for (const n of op.nodes) {
        if (!NODES.has(n)) failWrap(`${label}: node '${n}' not in N1..N13`);
      }

    if (typeof op.notes !== "string")
      failWrap(`${label}: notes must be a string`);
  }

  for (const id of EXPECTED_OPS) {
    if (!seenIds.has(id))
      failWrap(`missing operation '${id}' in operations.json`);
  }

  // Scan backend/src/**/*.rs for markers. The scan root defaults to
  // <root>/backend/src and can be overridden (second argument after the
  // base); the scenario-yaml convention derives from it.
  const sourceDir = args[1]
    ? resolve(args[1])
    : join(repoRoot, "backend", "src");
  const backendSrc = sourceDir;
  function walkRs(dir: string, out: string[]): void {
    if (!existsSync(dir)) return;
    for (const name of readdirSync(dir)) {
      const full = join(dir, name);
      let st;
      try {
        st = statSync(full);
      } catch {
        continue;
      }
      if (st.isDirectory()) walkRs(full, out);
      else if (name.endsWith(".rs")) out.push(full);
    }
  }
  const rsFiles: string[] = [];
  walkRs(backendSrc, rsFiles);

  function collectFileMarkers(text: string): {
    opMarkers: string[];
    eventMarkers: string[];
  } {
    const lines = text.split("\n");
    const opMarkers: string[] = [];
    const eventMarkers: string[] = [];
    let block: string[] = [];
    for (const line of lines) {
      if (/^\s*#\[/.test(line)) {
        block.push(line);
        const opm = line.match(/quint::operation\(\s*"([^"]+)"\s*\)/);
        if (opm) {
          const testGated = block.some(
            (l) =>
              /cfg\s*\(\s*test\s*\)/.test(l) || /^\s*#\[\s*test\s*\]/.test(l),
          );
          if (!testGated) opMarkers.push(opm[1]);
        }
        const evm = line.match(/quint::event\(\s*"([^"]+)"\s*\)/);
        if (evm) {
          const testGated = block.some(
            (l) =>
              /cfg\s*\(\s*test\s*\)/.test(l) || /^\s*#\[\s*test\s*\]/.test(l),
          );
          if (!testGated) eventMarkers.push(evm[1]);
        }
        continue;
      }
      block = [];
      if (line.trim() === "") continue;
    }
    return { opMarkers, eventMarkers };
  }

  const markerOps = new Set<string>();
  const markerOpCounts = new Map<string, number>();

  for (const file of rsFiles) {
    const text = readFileSync(file, "utf8");
    const { opMarkers, eventMarkers } = collectFileMarkers(text);
    for (const id of opMarkers) {
      if (!EXPECTED_OPS.has(id))
        failWrap(
          `marker in ${relative(repoRoot, file)}: unknown operation '${id}'`,
        );
      else {
        markerOps.add(id);
        markerOpCounts.set(id, (markerOpCounts.get(id) ?? 0) + 1);
      }
    }
    for (const ev of eventMarkers) {
      if (eventNames.size > 0 && !eventNames.has(ev))
        failWrap(
          `marker in ${relative(repoRoot, file)}: unknown event '${ev}'`,
        );
    }
  }

  for (const id of EXPECTED_OPS) {
    if (!markerOps.has(id))
      failWrap(
        `operation '${id}' has no #[quint::operation(...)] marker on a real item in ${sourceDir}`,
      );
  }

  const ownedEvents = new Set<string>();
  for (const op of entries) {
    for (const e of op.events ?? []) ownedEvents.add(e);
  }
  if (eventsDoc && Array.isArray(eventsDoc.events)) {
    const seenEventNames = new Set<string>(
      eventsDoc.events
        .map((e: any) => e?.name)
        .filter((x: any): x is string => typeof x === "string"),
    );
    for (const name of seenEventNames) {
      const owned = ownedEvents.has(name);
      if (!owned && !EVENT_OWNER_EXEMPT.has(name)) {
        failWrap(
          `event '${name}' has no owning operation and is not in the exempt set`,
        );
      }
    }
    for (const name of EVENT_OWNER_EXEMPT.keys()) {
      if (!seenEventNames.has(name))
        failWrap(`exempt event '${name}' not present in events.json`);
    }
    for (const name of EVENT_OWNER_EXEMPT.keys()) {
      if (ownedEvents.has(name))
        warn(
          `event '${name}' is exempt but also owned by an operation (stale exemption)`,
        );
    }
  }

  const tagged = existsSync(base)
    ? collectOperationTags(base)
    : new Set<string>();

  const guardedOps = entries
    .filter((o: any) => o?.status === "guarded")
    .map((o: any) => o.id);
  const guardedUntagged = guardedOps.filter((id: string) => !tagged.has(id));
  if (guardedUntagged.length > 0) {
    for (const id of guardedUntagged)
      failWrap(`guarded OP '${id}' has no model @operation tag`);
  } else {
    console.log("report: all guarded OPs have a model @operation tag");
  }

  if (failed) {
    console.error("operations-check: FAILED");
    return 1;
  }

  const countSummary = [...EXPECTED_OPS]
    .map((id) => `${id}=${markerOpCounts.get(id) ?? 0}`)
    .join(" ");
  console.log(
    `operations-check: OK (${seenIds.size} operations, ${markerOps.size} marked, ${rsFiles.length} rs files scanned)`,
  );
  console.log(`  marker counts: ${countSummary}`);
  console.log(
    `  model tags: ${tagged.size} OPs tagged; guarded untagged: ${guardedUntagged.length}`,
  );

  console.log("  coverage matrix:");
  for (const id of EXPECTED_OPS) {
    const rows = coverageRows.filter((row) => row.op === id);
    if (rows.length === 0) {
      console.log(`    ${id}: (no properties)`);
      continue;
    }
    const cells = rows.map((row) => `${row.model} -> ${row.test ?? "NO TEST"}`);
    console.log(`    ${id}: ${cells.join("; ")}`);
  }
  const tested = coverageRows.filter((row) => row.test !== undefined).length;
  console.log(`  properties with test: ${tested}/${coverageRows.length}`);
  return 0;
}
