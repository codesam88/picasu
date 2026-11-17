#!/usr/bin/env node
import { run as runEvents } from "../checks/events.ts";
import { run as runCatalog } from "../checks/catalog.ts";
import { run as runOps } from "../checks/ops.ts";
import { run as runSpecs } from "../checks/specs.ts";
import { run as runSteps } from "../checks/step.ts";
import { docs as runDocs, docCheck as runDocCheck } from "../checks/doc.ts";

const USAGE = `Usage: check.ts <subcommand> [root-or-specs-dir] [args]
Subcommands:
  events|catalog|ops [root-or-specs-dir] [source-dir?]
  specs [root-or-specs-dir] [--generate]
  steps <pipeline-dir-or-root>
  doc-check [root-or-specs-dir] <committed-file> <run-source.qnt>...
  docs    [root-or-specs-dir] <out-file> <run-source.qnt>...
  all     [root-or-specs-dir] [committed-file? [run-source.qnt...]]   # doc-check skipped when no file given`;

async function main(): Promise<number> {
  const argv = process.argv.slice(2);
  const sub = argv[0];
  const rest = argv.slice(1);

  switch (sub) {
    case "events":
      return runEvents(rest);
    case "catalog":
      return runCatalog(rest);
    case "ops":
      return runOps(rest);
    case "specs":
      return runSpecs(rest);
    case "steps":
      return runSteps(rest);
    case "doc-check":
      return runDocCheck(rest);
    case "docs":
      return runDocs(rest);
    case "all": {
      const base = rest[0] ?? ".";
      const docFile = rest[1];
      const runSources = rest.slice(2);
      let code = 0;
      const steps: { name: string; fn: (a: string[]) => number }[] = [
        { name: "events", fn: runEvents },
        { name: "catalog", fn: runCatalog },
        { name: "ops", fn: runOps },
        { name: "specs", fn: runSpecs },
        { name: "steps", fn: runSteps },
      ];
      if (docFile) steps.push({ name: "doc-check", fn: runDocCheck });
      else
        console.log(
          "===== doc-check =====\nskipped: no committed-file argument given",
        );
      for (const s of steps) {
        console.log(`===== ${s.name} =====`);
        const a =
          s.name === "doc-check" ? [base, docFile, ...runSources] : [base];
        const c = s.fn(a);
        if (c !== 0) code = 1;
      }
      return code;
    }
    default:
      console.error(USAGE);
      return 2;
  }
}

main()
  .then((c) => process.exit(c))
  .catch((e) => {
    console.error(e);
    process.exit(1);
  });
