import { existsSync } from "node:fs";
import { resolveBase, loadJson, fail } from "../lib/common.ts";
import { validate, SchemaError } from "../lib/schema.ts";
import { loadSchema } from "../lib/catalog.ts";

export function run(args: string[]): number {
  let strict = false;
  const positional: string[] = [];
  for (const a of args) {
    if (a === "--strict") strict = true;
    else positional.push(a);
  }

  const base = resolveBase(positional[0], "events.json");
  const eventsPath = base + "/events.json"; // placeholder, use join? we'll fix
  void eventsPath;

  let failed = false;
  const failWrap = (msg: string) => {
    fail(msg);
    failed = true;
  };

  let eventsDoc: any;
  let schema: any;
  try {
    eventsDoc = loadJson(base + "/events.json");
  } catch {
    process.exit(1);
  }
  // loadJson already exits on failure; but typing
  try {
    schema = loadSchema(base);
  } catch {
    process.exit(1);
  }

  const defs = schema.definitions ?? {};
  const eventDef = defs.event ?? {};
  const eventProps = eventDef.properties ?? {};
  const nameEnum = eventProps.name?.enum ?? [];
  const affectedEnum = eventProps.affected_state?.items?.enum ?? [];
  const requiredFields = eventDef.required ?? [];
  const variantRequired = defs.variant?.required ?? [];

  const STATUSES = new Set(["established", "proposed"]);
  const FAULT_STATUSES = new Set(["covered", "open", "tbd"]);
  function isTodoCodeRef(ref: any) {
    return typeof ref === "string" && ref.includes("TODO");
  }

  const entries: any[] = Array.isArray(eventsDoc.events)
    ? eventsDoc.events
    : [];
  const seenNames = new Set<string>();
  const todoRefs: string[] = [];

  if (typeof eventsDoc.version !== "number")
    failWrap("events.json: missing/invalid version");
  if (
    typeof eventsDoc.description !== "string" ||
    eventsDoc.description.trim() === ""
  ) {
    failWrap("events.json: missing/empty top-level description");
  }

  for (let i = 0; i < entries.length; i++) {
    const ev = entries[i];
    const label = `events[${i}]${ev && ev.name ? ` (${ev.name})` : ""}`;

    // schema shape validation
    try {
      const errs = validate(ev, eventDef, schema);
      for (const e of errs) failWrap(`${label}: ${e}`);
    } catch (e: any) {
      failWrap(`${label}: schema error: ${e?.message ?? e}`);
    }

    // required fields (redundant but keep messages)
    for (const f of requiredFields) {
      if (ev == null || !(f in ev))
        failWrap(`${label}: missing required field '${f}'`);
    }
    if (ev == null) continue;

    if (!nameEnum.includes(ev.name))
      failWrap(`${label}: name '${ev.name}' not in schema enum`);
    if (seenNames.has(ev.name))
      failWrap(`${label}: duplicate event name '${ev.name}'`);
    seenNames.add(ev.name);

    if (!STATUSES.has(ev.status))
      failWrap(
        `${label}: status '${ev.status}' not in ${[...STATUSES].join("/")}`,
      );

    if (typeof ev.description !== "string" || ev.description.trim() === "") {
      failWrap(`${label}: description missing or empty`);
    }

    if (
      !Array.isArray(ev.effects) ||
      ev.effects.length < 1 ||
      !ev.effects.every((s: any) => typeof s === "string" && s.trim() !== "")
    ) {
      failWrap(
        `${label}: effects must be a non-empty array of non-empty strings`,
      );
    }

    if (!Array.isArray(ev.faults)) {
      failWrap(`${label}: faults must be an array`);
    } else
      for (let f = 0; f < ev.faults.length; f++) {
        const fl = ev.faults[f];
        const flabel = `${label}.faults[${f}]`;
        if (fl == null || typeof fl !== "object" || Array.isArray(fl)) {
          failWrap(`${flabel}: must be an object`);
          continue;
        }
        for (const key of ["fault", "consequence"]) {
          if (typeof fl[key] !== "string" || fl[key].trim() === "")
            failWrap(`${flabel}: missing/empty '${key}'`);
        }
        if (!FAULT_STATUSES.has(fl.status))
          failWrap(
            `${flabel}: status '${fl.status}' not in ${[...FAULT_STATUSES].join("/")}`,
          );
      }

    if (!Array.isArray(ev.affected_state))
      failWrap(`${label}: affected_state must be array`);
    else
      for (const s of ev.affected_state) {
        if (!affectedEnum.includes(s))
          failWrap(`${label}: affected_state item '${s}' not in enum`);
      }

    if (isTodoCodeRef(ev.code_ref)) todoRefs.push(`${ev.name}: ${ev.code_ref}`);

    if (ev.variants != null) {
      if (!Array.isArray(ev.variants))
        failWrap(`${label}: variants must be array`);
      else
        for (let v = 0; v < ev.variants.length; v++) {
          const vr = ev.variants[v];
          const vlabel = `${label}.variants[${v}]`;
          for (const f of variantRequired) {
            if (vr == null || !(f in vr))
              failWrap(`${vlabel}: missing required field '${f}'`);
          }
          if (vr == null) continue;
          if (isTodoCodeRef(vr.code_ref))
            todoRefs.push(`${ev.name}/${vr.name}: ${vr.code_ref}`);
        }
    }
  }

  if (entries.length !== nameEnum.length) {
    failWrap(
      `events count ${entries.length} != schema enum count ${nameEnum.length}`,
    );
  }
  for (const n of nameEnum) {
    if (!seenNames.has(n)) failWrap(`missing event '${n}'`);
  }

  if (todoRefs.length > 0) {
    console.warn(`TODO code_refs (${todoRefs.length}):`);
    for (const t of todoRefs) console.warn(`  - ${t}`);
    if (strict) {
      failWrap(`--strict: ${todoRefs.length} TODO code_ref(s) present`);
    }
  }

  if (failed) {
    console.error("events-check: FAILED");
    return 1;
  }
  console.log(
    `events-check: OK (${entries.length} events, ${todoRefs.length} TODO code_refs)`,
  );
  return 0;
}
