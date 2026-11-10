import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import {
  applyVersion,
  packageRelativePaths,
  readPackage,
  workspaceVersion,
} from "../../versions.mjs";
import { binarySubpath, packageName, resolveBinary } from "./yuzuko.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");

test("maps each published target", () => {
  assert.equal(packageName("darwin", "arm64"), "@yuzuko/cli-darwin-arm64");
  assert.equal(packageName("darwin", "x64"), "@yuzuko/cli-darwin-x64");
  assert.equal(packageName("linux", "arm64"), "@yuzuko/cli-linux-arm64");
  assert.equal(packageName("linux", "x64"), "@yuzuko/cli-linux-x64");
  assert.equal(packageName("win32", "x64"), "@yuzuko/cli-win32-x64");
  assert.equal(packageName("win32", "arm64"), null);
  assert.equal(packageName("freebsd", "x64"), null);
});

test("points at the executable inside the optional package", () => {
  assert.equal(binarySubpath("win32"), "bin/yuzuko.exe");
  assert.equal(binarySubpath("linux"), "bin/yuzuko");
  assert.equal(binarySubpath("darwin"), "bin/yuzuko");
});

test("reports an unsupported platform", () => {
  const found = resolveBinary("freebsd", "x64", () => {
    throw new Error("resolve should not run");
  });
  assert.match(found.error, /freebsd x64/);
});

test("reports a missing platform package", () => {
  const found = resolveBinary("linux", "x64", () => {
    throw new Error("cannot find module");
  });
  assert.match(found.error, /@yuzuko\/cli-linux-x64/);
  assert.match(found.error, /optionalDependencies/);
});

test("resolves the executable published in the optional package", () => {
  const binary = path.join("/opt", "pkg", "bin", "yuzuko.exe");
  const found = resolveBinary("win32", "x64", (id) => {
    assert.equal(id, "@yuzuko/cli-win32-x64/bin/yuzuko.exe");
    return binary;
  });
  assert.equal(found.path, binary);
});

test("package versions are generated from Cargo.toml", () => {
  const version = workspaceVersion(root);
  assert.match(version, /^\d+\.\d+\.\d+$/);
  for (const relative of packageRelativePaths(root)) {
    const source = readPackage(root, relative);
    assert.equal(source.version, undefined);
    for (const range of Object.values(source.optionalDependencies ?? {})) {
      assert.equal(range, "");
    }
    const generated = applyVersion(source, version);
    assert.deepEqual(Object.keys(generated).slice(0, 2), ["name", "version"]);
    assert.equal(generated.version, version);
    for (const range of Object.values(generated.optionalDependencies ?? {})) {
      assert.equal(range, version);
    }
  }
  const cli = readPackage(root, "npm/cli/package.json");
  assert.equal(cli.type, "module");
  assert.equal(cli.engines.node, ">=22");
  assert.equal(cli.scripts, undefined);
});
