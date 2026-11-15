import { resolveBase, loadJson, fail } from "../lib/common.ts";
import { validate, SchemaError } from "../lib/schema.ts";
import { loadSchema } from "../lib/catalog.ts";

function nonEmptyString(s: any): boolean {
  return typeof s === "string" && s.trim() !== "";
}

export function run(args: string[]): number {
  const arg = args[0] ?? ".";
  const base = resolveBase(arg, "goals.json");

  let failed = false;
  const failWrap = (msg: string) => {
    fail(msg);
    failed = true;
  };

  function load(name: string): any {
    return loadJson(base + "/" + name);
  }

  const goalsDoc = load("goals.json");
  const assumptionsDoc = load("assumptions.json");
  const requirementsDoc = load("requirements.json");
  const scopeDoc = load("scope.json");
  const catalogSchema = load("catalog.schema.json");

  for (const [name, doc] of [
    ["goals.json", goalsDoc],
    ["requirements.json", requirementsDoc],
    ["assumptions.json", assumptionsDoc],
    ["scope.json", scopeDoc],
  ]) {
    if (!nonEmptyString(doc.description))
      failWrap(`${name}: missing/empty top-level description`);
  }
  if (!nonEmptyString(scopeDoc.modeled_scope))
    failWrap("scope.json: missing/empty modeled_scope");

  const goalIds = catalogSchema.definitions.goal.properties.id.enum;
  const decisions = catalogSchema.definitions.goal.properties.decision.enum;
  const anchorKinds = catalogSchema.definitions.anchor.properties.kind.enum;
  const assumptionIdsEnum =
    catalogSchema.definitions.assumption.properties.id.enum;
  const statuses = catalogSchema.definitions.assumption.properties.status.enum;
  const requirementIdsEnum =
    catalogSchema.definitions.requirement.properties.id.enum;
  const requirementStatuses =
    catalogSchema.definitions.requirement.properties.status.enum;
  const scopeIdsEnum = catalogSchema.definitions.scope.properties.id.enum;

  const assumptionList: any[] = Array.isArray(assumptionsDoc.assumptions)
    ? assumptionsDoc.assumptions
    : [];
  const assumptionIds = new Set<string>();
  for (let i = 0; i < assumptionList.length; i++) {
    const a = assumptionList[i];
    const label = `assumptions[${i}]${a && a.id ? ` (${a.id})` : ""}`;
    if (a == null || typeof a !== "object") {
      failWrap(`${label}: not an object`);
      continue;
    }
    // schema shape
    try {
      const errs = validate(
        a,
        catalogSchema.definitions.assumption,
        catalogSchema,
      );
      for (const e of errs) failWrap(`${a?.id ?? label}: ${e}`);
    } catch (e: any) {
      failWrap(`${label}: schema error: ${e?.message ?? e}`);
    }
    if (!assumptionIdsEnum.includes(a.id))
      failWrap(`${label}: id '${a.id}' not in schema enum`);
    if (assumptionIds.has(a.id)) failWrap(`${label}: duplicate id`);
    assumptionIds.add(a.id);
    for (const f of ["title", "owner"]) {
      if (!nonEmptyString(a[f])) failWrap(`${a.id}: missing/empty '${f}'`);
    }
    if (!statuses.includes(a.status)) {
      failWrap(`${a.id}: status '${a.status}' not in ${statuses.join("/")}`);
    }
  }

  const scopeList: any[] = Array.isArray(scopeDoc.scopes)
    ? scopeDoc.scopes
    : [];
  const scopeIds = new Set<string>();
  for (let i = 0; i < scopeList.length; i++) {
    const s = scopeList[i];
    const label = `scopes[${i}]${s && s.id ? ` (${s.id})` : ""}`;
    if (s == null || typeof s !== "object") {
      failWrap(`${label}: not an object`);
      continue;
    }
    try {
      const errs = validate(s, catalogSchema.definitions.scope, catalogSchema);
      for (const e of errs) failWrap(`${s?.id ?? label}: ${e}`);
    } catch (e: any) {
      failWrap(`${label}: schema error: ${e?.message ?? e}`);
    }
    if (!scopeIdsEnum.includes(s.id))
      failWrap(`${label}: id '${s.id}' not in schema enum`);
    if (scopeIds.has(s.id)) failWrap(`${label}: duplicate id`);
    scopeIds.add(s.id);
    for (const f of ["title", "bound", "why_safe", "lifting"]) {
      if (!nonEmptyString(s[f])) failWrap(`${s.id}: missing/empty '${f}'`);
    }
    if ("provisional" in s && typeof s.provisional !== "boolean") {
      failWrap(`${s.id}: provisional must be a boolean when present`);
    }
  }

  const goalList: any[] = Array.isArray(goalsDoc.goals) ? goalsDoc.goals : [];
  const seenGoals = new Set<string>();
  for (let i = 0; i < goalList.length; i++) {
    const g = goalList[i];
    const label = `goals[${i}]${g && g.id ? ` (${g.id})` : ""}`;
    if (g == null || typeof g !== "object") {
      failWrap(`${label}: not an object`);
      continue;
    }
    try {
      const errs = validate(g, catalogSchema.definitions.goal, catalogSchema);
      for (const e of errs) failWrap(`${g?.id ?? label}: ${e}`);
    } catch (e: any) {
      failWrap(`${label}: schema error: ${e?.message ?? e}`);
    }
    if (!goalIds.includes(g.id))
      failWrap(`${label}: id '${g.id}' not in schema enum`);
    if (seenGoals.has(g.id)) failWrap(`${label}: duplicate id`);
    seenGoals.add(g.id);
    if (!decisions.includes(g.decision)) {
      failWrap(
        `${g.id}: decision '${g.decision}' not in ${decisions.join("/")}`,
      );
    }
    for (const f of ["title", "description"]) {
      if (!nonEmptyString(g[f])) failWrap(`${g.id}: missing/empty '${f}'`);
    }
    if (!Array.isArray(g.anchors))
      failWrap(`${g.id}: anchors must be an array`);
    else {
      if (g.decision === "committed" && g.anchors.length === 0) {
        failWrap(
          `${g.id}: committed goal has no anchor (predicate, code or test)`,
        );
      }
      for (let n = 0; n < g.anchors.length; n++) {
        const an = g.anchors[n];
        if (an == null || !anchorKinds.includes(an.kind)) {
          failWrap(
            `${g.id}.anchors[${n}]: kind '${an?.kind}' not in ${anchorKinds.join("/")}`,
          );
        }
        if (!nonEmptyString(an?.ref))
          failWrap(`${g.id}.anchors[${n}]: missing/empty ref`);
      }
    }
    for (const ref of g.assumptions ?? []) {
      if (!assumptionIds.has(ref))
        failWrap(`${g.id}: assumption ref '${ref}' not in assumptions.json`);
    }
  }

  const committedGoals = new Set(
    goalList
      .filter((g) => g && g.id && g.decision === "committed")
      .map((g) => g.id),
  );
  const requirementList: any[] = Array.isArray(requirementsDoc.requirements)
    ? requirementsDoc.requirements
    : [];
  const requirementIds = new Set<string>();
  for (let i = 0; i < requirementList.length; i++) {
    const r = requirementList[i];
    const label = `requirements[${i}]${r && r.id ? ` (${r.id})` : ""}`;
    if (r == null || typeof r !== "object") {
      failWrap(`${label}: not an object`);
      continue;
    }
    try {
      const errs = validate(
        r,
        catalogSchema.definitions.requirement,
        catalogSchema,
      );
      for (const e of errs) failWrap(`${r?.id ?? label}: ${e}`);
    } catch (e: any) {
      failWrap(`${label}: schema error: ${e?.message ?? e}`);
    }
    if (!requirementIdsEnum.includes(r.id))
      failWrap(`${label}: id '${r.id}' not in schema enum`);
    if (requirementIds.has(r.id)) failWrap(`${label}: duplicate id`);
    requirementIds.add(r.id);
    for (const f of ["title", "statement", "owner", "gate", "implementation"]) {
      if (!nonEmptyString(r[f])) failWrap(`${r.id}: missing/empty '${f}'`);
    }
    if (
      !Array.isArray(r.derived_from) ||
      r.derived_from.length < 1 ||
      !r.derived_from.every((id: any) => goalIds.includes(id))
    ) {
      failWrap(`${r.id}: derived_from must be a non-empty array of goal ids`);
    } else {
      for (const gid of r.derived_from) {
        if (!committedGoals.has(gid)) {
          failWrap(
            `${r.id}: derives from ${gid}, which is not a committed goal`,
          );
        }
      }
    }
    if (!requirementStatuses.includes(r.status)) {
      failWrap(
        `${r.id}: status '${r.status}' not in ${requirementStatuses.join("/")}`,
      );
    }
    if (!Array.isArray(r.tests) || !r.tests.every(nonEmptyString)) {
      failWrap(`${r.id}: tests must be an array of non-empty test names`);
    } else if (r.status === "verified" && r.tests.length === 0) {
      failWrap(`${r.id}: verified requirement has no gate test`);
    }
    if ("limits" in r && !nonEmptyString(r.limits)) {
      failWrap(`${r.id}: limits must be a non-empty string when present`);
    }
    const allowed = new Set([
      "id",
      "title",
      "statement",
      "derived_from",
      "owner",
      "gate",
      "status",
      "implementation",
      "tests",
      "limits",
    ]);
    for (const key of Object.keys(r)) {
      if (!allowed.has(key)) failWrap(`${r.id}: unknown field '${key}'`);
    }
  }
  for (const gid of committedGoals) {
    if (!requirementList.some((r) => (r.derived_from ?? []).includes(gid))) {
      failWrap(`committed goal ${gid} derives no requirements`);
    }
  }

  if (failed) {
    console.error("catalog-check: FAILED");
    return 1;
  }
  console.log(
    `catalog-check: OK (${goalList.length} goals, ${requirementIds.size} requirements, ${assumptionIds.size} assumptions, ${scopeIds.size} scopes)`,
  );
  return 0;
}
