/**
 * Compare yuzuko.d.ts with the installed @types/web and @webgpu/types.
 *
 *   node lint-web-types.mjs
 *   node lint-web-types.mjs --web node_modules/@types/web/index.d.ts
 *   node lint-web-types.mjs --webgpu node_modules/@webgpu/types/dist/index.d.ts
 *   node lint-web-types.mjs --yuzuko ../../yuzuko.d.ts --allow allowlist.json
 *
 * The default paths are the copies installed for this workspace.
 * `npm install` at the repository root hoists them to the root `node_modules`.
 * `@webgpu/types` publishes its declarations at the `types` field, `dist/index.d.ts`.
 *
 * Exit 0 when yuzuko.d.ts already follows both pinned files. Exit 1 and print
 * each reference member or spelling that yuzuko.d.ts does not follow.
 */
import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { allowFromJson, compareTexts } from "./compare.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const options = parseArgs(process.argv.slice(2));
const yuzukoPath = path.resolve(options.yuzuko ?? path.join(here, "../../yuzuko.d.ts"));
const webPath = path.resolve(options.web ?? installedTypes("@types/web"));
const webgpuPath = path.resolve(options.webgpu ?? installedTypes("@webgpu/types"));
const allowPath = path.resolve(options.allow ?? path.join(here, "allowlist.json"));

const yuzukoText = fs.readFileSync(yuzukoPath, "utf8");
const allowJson = JSON.parse(fs.readFileSync(allowPath, "utf8"));
const webStatus = report(
  compareTexts(yuzukoText, fs.readFileSync(webPath, "utf8"), allowFromJson(allowJson)),
  "@types/web",
  packageVersion(webPath, "@types/web"),
  webPath,
);
console.log("");
const webgpuStatus = report(
  compareTexts(
    yuzukoText,
    fs.readFileSync(webgpuPath, "utf8"),
    allowFromJson(allowJson, { webgpu: true }),
    "@webgpu/types",
  ),
  "@webgpu/types",
  packageVersion(webgpuPath, "@webgpu/types"),
  webgpuPath,
);
process.exit(webStatus || webgpuStatus ? 1 : 0);

function report(result, label, version, file) {
  const versionText = version ? `${label} ${version}` : file;
  console.log(`web-types: ${path.basename(yuzukoPath)} vs ${versionText}`);
  console.log(`compared ${result.stats.types} types, ${result.stats.values} values, ${result.stats.functions} functions`);
  if (result.findings.length === 0) {
    console.log("no differences");
    return 0;
  }
  console.log("");
  for (const finding of result.findings) {
    console.log(finding.path);
    console.log(`  ${kindLabel(finding.kind, label)}`);
    if (finding.web) console.log(`  ${label}: ${finding.web}`);
    if (finding.yuzuko) console.log(`  yuzuko: ${finding.yuzuko}`);
    if (finding.note) console.log(`  ${finding.note}`);
    console.log("");
  }
  console.log(`${result.findings.length} ${result.findings.length === 1 ? "difference" : "differences"}`);
  return 1;
}

function kindLabel(kind, label) {
  if (kind === "missing") return "missing in yuzuko.d.ts";
  if (kind === "overload") return "overload missing in yuzuko.d.ts";
  return `${label} is more specific`;
}

function installedTypes(packageName) {
  const manifestPath = require.resolve(`${packageName}/package.json`);
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  return path.join(path.dirname(manifestPath), manifest.types ?? manifest.typings ?? "index.d.ts");
}

function packageVersion(file, packageName) {
  let dir = path.dirname(file);
  for (let depth = 0; depth < 4; depth += 1) {
    const manifestPath = path.join(dir, "package.json");
    if (fs.existsSync(manifestPath)) {
      const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
      if (manifest.name === packageName) return manifest.version ?? null;
    }
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  return null;
}

function parseArgs(argv) {
  const out = {};
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--yuzuko" || arg === "--web" || arg === "--webgpu" || arg === "--allow") {
      out[arg.slice(2)] = argv[index + 1];
      index += 1;
    } else if (arg === "--help") {
      console.log("usage: node lint-web-types.mjs [--yuzuko file] [--web file] [--webgpu file] [--allow file]");
      process.exit(0);
    } else {
      console.error(`unknown argument: ${arg}`);
      process.exit(2);
    }
  }
  return out;
}
