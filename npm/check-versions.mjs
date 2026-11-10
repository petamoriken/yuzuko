import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  packageRelativePaths,
  readPackage,
  workspaceVersion,
  writeVersions,
} from "./versions.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
let version;
try {
  version = workspaceVersion(root);
} catch (error) {
  console.error(error.message);
  process.exit(1);
}

const tag = process.argv[2];
if (tag !== undefined && tag !== `v${version}`) {
  console.error(`tag ${tag} does not match workspace version ${version}`);
  process.exit(1);
}

const platformNames = packageRelativePaths(root)
  .filter((relative) => relative.startsWith("npm/platforms/"))
  .map((relative) => readPackage(root, relative).name)
  .sort();

for (const relative of packageRelativePaths(root)) {
  const json = readPackage(root, relative);
  if (relative.startsWith("npm/platforms/")) {
    const dir = relative.split("/")[2];
    if (json.name !== `@yuzuko/cli-${dir}`) {
      console.error(`${relative} name is ${json.name}`);
      process.exit(1);
    }
    if (json.bin) {
      console.error(`${relative} must not set bin; that name belongs to @yuzuko/cli`);
      process.exit(1);
    }
    if (json.preferUnplugged !== true) {
      console.error(`${relative} must set preferUnplugged`);
      process.exit(1);
    }
  }
}

const cli = readPackage(root, "npm/cli/package.json");
if (cli.name !== "@yuzuko/cli") {
  console.error("npm/cli package name is not @yuzuko/cli");
  process.exit(1);
}
if (cli.type !== "module") {
  console.error("@yuzuko/cli must be an ES module package");
  process.exit(1);
}
if (cli.scripts) {
  console.error("@yuzuko/cli must not run an install script");
  process.exit(1);
}
if (cli.engines?.node !== ">=22") {
  console.error("@yuzuko/cli must require Node.js 22 or newer");
  process.exit(1);
}
const optional = Object.keys(cli.optionalDependencies ?? {}).sort();
if (optional.join("\n") !== platformNames.join("\n")) {
  console.error("optionalDependencies do not match npm/platforms");
  process.exit(1);
}

const types = readPackage(root, "npm/types/package.json");
if (types.name !== "@yuzuko/types" || types.types !== "yuzuko.d.ts") {
  console.error("@yuzuko/types must name yuzuko.d.ts as its types entry");
  process.exit(1);
}

writeVersions(root);
process.stdout.write(`${version}\n`);
