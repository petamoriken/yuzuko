/**
 * Compare yuzuko.d.ts with the installed @types/web.
 *
 *   node lint.mjs
 *   node lint.mjs --web node_modules/@types/web/index.d.ts
 *   node lint.mjs --yuzuko ../../yuzuko.d.ts --allow allowlist.json
 *
 * Exit 0 when yuzuko.d.ts already follows the pinned types. Exit 1 and print
 * each @types/web member or spelling that yuzuko.d.ts does not follow.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { allowFromJson, compareTexts } from "./compare.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const options = parseArgs(process.argv.slice(2));
const yuzukoPath = path.resolve(options.yuzuko ?? path.join(here, "../../yuzuko.d.ts"));
const webPath = path.resolve(options.web ?? path.join(here, "node_modules/@types/web/index.d.ts"));
const allowPath = path.resolve(options.allow ?? path.join(here, "allowlist.json"));

const yuzukoText = fs.readFileSync(yuzukoPath, "utf8");
const webText = fs.readFileSync(webPath, "utf8");
const allow = allowFromJson(JSON.parse(fs.readFileSync(allowPath, "utf8")));
const version = webPackageVersion(webPath);
const result = compareTexts(yuzukoText, webText, allow);

const versionText = version ? `@types/web ${version}` : webPath;
console.log(`web-types: ${path.basename(yuzukoPath)} vs ${versionText}`);
console.log(`compared ${result.stats.types} types, ${result.stats.values} values, ${result.stats.functions} functions`);

if (result.findings.length === 0) {
  console.log("no differences");
  process.exit(0);
}

console.log("");
for (const finding of result.findings) {
  console.log(finding.path);
  console.log(`  ${label(finding.kind)}`);
  if (finding.web) console.log(`  @types/web: ${finding.web}`);
  if (finding.yuzuko) console.log(`  yuzuko:     ${finding.yuzuko}`);
  if (finding.note) console.log(`  ${finding.note}`);
  console.log("");
}
console.log(`${result.findings.length} ${result.findings.length === 1 ? "difference" : "differences"}`);
process.exit(1);

function label(kind) {
  if (kind === "missing") return "missing in yuzuko.d.ts";
  if (kind === "overload") return "overload missing in yuzuko.d.ts";
  return "@types/web is more specific";
}

function webPackageVersion(file) {
  const packagePath = path.join(path.dirname(file), "package.json");
  if (!fs.existsSync(packagePath)) return null;
  const manifest = JSON.parse(fs.readFileSync(packagePath, "utf8"));
  return manifest.name === "@types/web" ? manifest.version : null;
}

function parseArgs(argv) {
  const out = {};
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--yuzuko" || arg === "--web" || arg === "--allow") {
      out[arg.slice(2)] = argv[index + 1];
      index += 1;
    } else if (arg === "--help") {
      console.log("usage: node lint.mjs [--yuzuko file] [--web file] [--allow file]");
      process.exit(0);
    } else {
      console.error(`unknown argument: ${arg}`);
      process.exit(2);
    }
  }
  return out;
}
