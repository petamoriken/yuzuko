/**
 * Typecheck tests, examples, and the ambient declarations in the root tsconfig.
 *
 *   node lint-types.mjs
 *   node lint-types.mjs --tsconfig ../../tsconfig.json
 *
 * Exit 0 when that program has no errors outside the syntax fixtures
 * `tests/defer/invalid.json` and `tests/source/bad.ts`.
 */
import path from "node:path";
import { fileURLToPath } from "node:url";
import { API } from "typescript/sync";
import { checkProject, formatDiagnostic } from "./check.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "../..");
const options = parseArgs(process.argv.slice(2));
const tsconfig = path.resolve(options.tsconfig ?? path.join(repo, "tsconfig.json"));

const api = new API();
try {
  const result = checkProject(api, tsconfig);
  console.log(`test-types: checked ${result.fileCount} files`);
  if (result.errors.length === 0) {
    console.log("no errors");
  } else {
    console.log("");
    for (const diagnostic of result.errors) {
      console.log(formatDiagnostic(diagnostic, repo));
      console.log("");
    }
    const count = result.errors.length;
    console.log(`${count} ${count === 1 ? "error" : "errors"}`);
    process.exitCode = 1;
  }
} finally {
  api[Symbol.dispose]();
}

/**
 * @param {string[]} argv
 */
function parseArgs(argv) {
  /** @type {{ tsconfig?: string }} */
  const out = {};
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--tsconfig") {
      out.tsconfig = argv[index + 1];
      index += 1;
    } else if (arg === "--help") {
      console.log("usage: node lint-types.mjs [--tsconfig file]");
      process.exit(0);
    } else {
      console.error(`unknown argument: ${arg}`);
      process.exit(2);
    }
  }
  return out;
}
