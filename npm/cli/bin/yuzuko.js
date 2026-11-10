#!/usr/bin/env node

import { spawn } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const packages = {
  "darwin arm64": "@yuzuko/cli-darwin-arm64",
  "darwin x64": "@yuzuko/cli-darwin-x64",
  "linux arm64": "@yuzuko/cli-linux-arm64",
  "linux x64": "@yuzuko/cli-linux-x64",
  "win32 x64": "@yuzuko/cli-win32-x64",
};

export function packageName(platform, arch) {
  return packages[`${platform} ${arch}`] ?? null;
}

export function binarySubpath(platform) {
  return platform === "win32" ? "bin/yuzuko.exe" : "bin/yuzuko";
}

// `resolve` returns the executable path and throws when that file is absent.
// npm installs one optional dependency, and that package contains the executable.
export function resolveBinary(platform, arch, resolve) {
  const name = packageName(platform, arch);
  if (name === null) {
    return {
      error: `Yuzuko does not publish a binary for ${platform} ${arch}.`,
    };
  }
  try {
    return { path: resolve(`${name}/${binarySubpath(platform)}`) };
  } catch {
    return {
      error:
        `The optional package ${name} is not installed. ` +
        `@yuzuko/cli loads its binary through optionalDependencies. ` +
        `Install without --omit=optional.`,
    };
  }
}

function main() {
  const found = resolveBinary(process.platform, process.arch, (specifier) =>
    fileURLToPath(import.meta.resolve(specifier)),
  );
  if (found.error) {
    console.error(found.error);
    process.exit(1);
  }
  const child = spawn(found.path, process.argv.slice(2), { stdio: "inherit" });
  child.on("error", (error) => {
    console.error(error.message);
    process.exit(1);
  });
  child.on("exit", (code) => {
    process.exit(code === null ? 1 : code);
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
