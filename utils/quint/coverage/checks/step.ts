// Step branch-completeness check.
//
// The proof's reachable set is defined by a step action's `any { ... }`
// branch list: an action declared in a reachable module but forgotten in the
// list silently narrows what `quint verify` and the MBT lane explore. This
// check derives the list of declared actions from the step module's import
// graph and fails for each one the branch list omits.
//
// Scope and limitations:
// - Only declarations matching `action <step...>: bool = any { ... }` are
//   treated as step actions; other `any`-valued declarations are ignored.
// - Imports are resolved from `import M.* from "./file"` (whole module) and
//   `import M.name from "./file"` (that one action only). Module-qualified
//   references (`M.name(...)`) are not scanned; no current model uses them.
// - Excluded from the required set: `init` and `init<Name>` entry actions
//   (the `--init` operands of a run or verify invocation, not scheduler
//   branches) and any name matching the step pattern (`step`, `stepAspects`),
//   which are the step actions themselves rather than branches.
// - A missing `Skip`-style stutter branch is not reported: stutter is a
//   modeling policy, not a completeness fact.
// - Every reachable `action` declaration is required as a branch; a helper
//   action deliberately left out of the step must not be declared in a
//   reachable module.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { resolveBase, fail } from "../lib/common.ts";
import { listQntFiles } from "../lib/qnt.ts";

interface ActionDecl {
  file: string;
  line: number;
}

interface StepAction {
  file: string;
  name: string;
  branches: number;
  refs: Set<string>;
}

const STEP_RE = /^\s*action\s+(step\w*)\s*:\s*bool\s*=\s*any\s*\{/gm;
const ACTION_RE = /^\s*action\s+([A-Za-z_]\w*)/gm;
const IMPORT_RE =
  /^\s*import\s+([A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*)\.(\*|[A-Za-z_]\w*)\s+from\s+"([^"]+)"/gm;

function isStepName(name: string): boolean {
  return /^step\w*$/.test(name);
}

// `init` and `init<Name>` are run/verify entry points (an `--init` operand),
// not branches the scheduler may pick.
function isInitName(name: string): boolean {
  return /^init\w*$/.test(name);
}

// Replace comments (and, unless `keepStrings`, double-quoted strings) with
// spaces while preserving newlines and byte offsets, so regex matches keep
// their line numbers and braces inside strings/comments do not affect block
// matching. Imports are parsed with strings kept, since the path is quoted.
function strip(text: string, keepStrings: boolean): string {
  let out = "";
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    const n = text[i + 1];
    if (c === "/" && n === "/") {
      while (i < text.length && text[i] !== "\n") {
        out += " ";
        i++;
      }
    } else if (c === "/" && n === "*") {
      out += "  ";
      i += 2;
      while (i < text.length && !(text[i] === "*" && text[i + 1] === "/")) {
        out += text[i] === "\n" ? "\n" : " ";
        i++;
      }
      if (i < text.length) {
        out += "  ";
        i += 2;
      }
    } else if (c === '"') {
      out += keepStrings ? c : " ";
      i++;
      while (i < text.length && text[i] !== '"') {
        if (text[i] === "\\") {
          out += keepStrings ? text.slice(i, i + 2) : "  ";
          i += 2;
          continue;
        }
        out += keepStrings ? text[i] : text[i] === "\n" ? "\n" : " ";
        i++;
      }
      if (i < text.length) {
        out += keepStrings ? '"' : " ";
        i++;
      }
    } else {
      out += c;
      i++;
    }
  }
  return out;
}

function stripNoise(text: string): string {
  return strip(text, false);
}

function lineAt(text: string, index: number): number {
  let line = 1;
  for (let i = 0; i < index && i < text.length; i++) {
    if (text[i] === "\n") line++;
  }
  return line;
}

// Body of the brace block opening at `open`, or null if it never closes.
function blockBody(text: string, open: number): string | null {
  let depth = 0;
  for (let i = open; i < text.length; i++) {
    if (text[i] === "{") depth++;
    else if (text[i] === "}") {
      depth--;
      if (depth === 0) return text.slice(open + 1, i);
    }
  }
  return null;
}

// Branch references: names applied to arguments (`Foo("p1")`) and bare names
// (`Skip`). Arguments are string literals in these blocks, already blanked.
function branchReferences(body: string): { refs: string[]; count: number } {
  const refs: string[] = [];
  for (const m of body.matchAll(/\b([A-Za-z_]\w*)\s*\(/g)) refs.push(m[1]);
  for (const m of body.matchAll(/\b([A-Za-z_]\w*)\b(?!\s*\()/g))
    refs.push(m[1]);
  return { refs, count: refs.length };
}

export function run(args: string[]): number {
  const dirArg = args[0];
  if (!dirArg) {
    console.error("steps: missing required pipeline-dir argument");
    return 2;
  }
  const base = resolveBase(dirArg, "events.json");
  if (!existsSync(base)) {
    fail(`cannot read directory ${base}`);
    return 1;
  }

  let failed = false;
  const readText = (path: string): string | null => {
    try {
      return readFileSync(path, "utf8");
    } catch {
      fail(`cannot read ${path}`);
      failed = true;
      return null;
    }
  };
  const display = (path: string): string => relative(base, path);

  const resolveImport = (fromFile: string, spec: string): string => {
    const p = resolve(dirname(fromFile), spec);
    return p.endsWith(".qnt") ? p : `${p}.qnt`;
  };

  // Declared actions reachable from `startAbs`: the file itself plus every
  // module it imports, transitively. A single-symbol import contributes only
  // that named action and does not pull in the target's imports.
  const collectReachable = (startAbs: string): Map<string, ActionDecl> => {
    const found = new Map<string, ActionDecl>();
    const visited = new Set<string>();

    const addActions = (abs: string, only?: string): void => {
      const text = readText(abs);
      if (text === null) return;
      for (const m of stripNoise(text).matchAll(ACTION_RE)) {
        const name = m[1];
        if (only !== undefined && name !== only) continue;
        if (!found.has(name)) {
          found.set(name, {
            file: display(abs),
            line: lineAt(text, m.index ?? 0),
          });
        }
      }
    };

    const visit = (abs: string): void => {
      if (visited.has(abs)) return;
      visited.add(abs);
      addActions(abs);
      const text = readText(abs);
      if (text === null) return;
      for (const m of strip(text, true).matchAll(IMPORT_RE)) {
        const target = resolveImport(abs, m[3]);
        const leaf = m[2];
        if (leaf === "*") visit(target);
        else addActions(target, leaf);
      }
    };

    visit(startAbs);
    return found;
  };

  const stepActions: StepAction[] = [];
  for (const f of listQntFiles(base)) {
    const abs = join(base, f);
    const text = readText(abs);
    if (text === null) continue;
    const stripped = stripNoise(text);
    for (const m of stripped.matchAll(STEP_RE)) {
      const open = (m.index ?? 0) + m[0].length - 1;
      const body = blockBody(stripped, open);
      if (body === null) {
        fail(
          `${display(abs)}:${lineAt(text, m.index ?? 0)}: step '${m[1]}' has an unbalanced \`any\` block`,
        );
        failed = true;
        continue;
      }
      const { refs, count } = branchReferences(body);
      stepActions.push({
        file: display(abs),
        name: m[1],
        branches: count,
        refs: new Set(refs),
      });
    }
  }

  let totalBranches = 0;
  for (const sa of stepActions) {
    const reachable = collectReachable(join(base, sa.file));
    const required: [string, ActionDecl][] = [];
    for (const [name, decl] of reachable) {
      if (isInitName(name) || isStepName(name)) continue;
      required.push([name, decl]);
    }
    for (const [name, decl] of required) {
      if (sa.refs.has(name)) continue;
      fail(
        `step '${sa.name}' in ${sa.file} is missing declared action '${name}' (declared in ${decl.file}:${decl.line})`,
      );
      failed = true;
    }
    totalBranches += sa.branches;
    console.log(
      `steps-check: step '${sa.name}' in ${sa.file}: ${sa.branches} branches, ${required.length} declared actions`,
    );
  }

  if (failed) {
    console.error("steps-check: FAILED");
    return 1;
  }
  if (stepActions.length === 0) {
    console.log(`steps-check: OK (no step actions in ${base})`);
    return 0;
  }
  console.log(
    `steps-check: OK (${stepActions.length} step actions, ${totalBranches} branches)`,
  );
  return 0;
}
