import fs from "node:fs";
import path from "node:path";

export function workspaceVersion(root) {
  const cargo = fs.readFileSync(path.join(root, "Cargo.toml"), "utf8");
  const match = cargo.match(/\[workspace\.package\][^[]*^version = "([^"]+)"/m);
  if (!match) {
    throw new Error("Cargo.toml has no workspace.package version");
  }
  return match[1];
}

export function packageRelativePaths(root) {
  const platformDirs = fs
    .readdirSync(path.join(root, "npm/platforms"), { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();
  return [
    ...platformDirs.map((dir) => `npm/platforms/${dir}/package.json`),
    "npm/types/package.json",
    "npm/cli/package.json",
  ];
}

// `version` is inserted after `name`. Optional dependency ranges are replaced
// with the same Cargo version. The repository copies leave both unset.
export function applyVersion(json, version) {
  const next = {};
  for (const [key, value] of Object.entries(json)) {
    if (key === "version") continue;
    next[key] = value;
    if (key === "name") next.version = version;
  }
  if (next.version === undefined) next.version = version;
  if (next.optionalDependencies) {
    next.optionalDependencies = Object.fromEntries(
      Object.keys(next.optionalDependencies).map((name) => [name, version]),
    );
  }
  return next;
}

export function readPackage(root, relative) {
  return JSON.parse(fs.readFileSync(path.join(root, relative), "utf8"));
}

export function writeVersions(root) {
  const version = workspaceVersion(root);
  for (const relative of packageRelativePaths(root)) {
    const json = applyVersion(readPackage(root, relative), version);
    fs.writeFileSync(
      path.join(root, relative),
      `${JSON.stringify(json, null, 2)}\n`,
    );
  }
  return version;
}
