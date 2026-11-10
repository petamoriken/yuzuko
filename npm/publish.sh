#!/usr/bin/env bash
# Copy release assets into the platform packages and publish them.
# TAG is v0.2.0. DIST holds the GitHub release asset names.
# PUBLISH=1 publishes to npm. Without it, the script only stages files.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

tag=${TAG:?TAG is required, for example v0.2.0}
dist=${DIST:?DIST is the directory of release assets}
version=$(node npm/check-versions.mjs "$tag")

copy() {
  local asset=$1 dir=$2 name=$3
  mkdir -p "npm/platforms/$dir/bin"
  cp "$dist/$asset" "npm/platforms/$dir/bin/$name"
  if [[ "$name" != *.exe ]]; then
    chmod +x "npm/platforms/$dir/bin/$name"
  fi
}

copy yuzuko-aarch64-apple-darwin darwin-arm64 yuzuko
copy yuzuko-x86_64-apple-darwin darwin-x64 yuzuko
copy yuzuko-aarch64-unknown-linux-gnu linux-arm64 yuzuko
copy yuzuko-x86_64-unknown-linux-gnu linux-x64 yuzuko
copy yuzuko-x86_64-pc-windows-msvc.exe win32-x64 yuzuko.exe
cp "$dist/yuzuko.d.ts" npm/types/yuzuko.d.ts

if [[ "${PUBLISH:-}" != "1" ]]; then
  echo "staged @yuzuko packages at $version"
  exit 0
fi

publish() {
  local name version_json fields
  fields=$(node --input-type=module <<'EOF'
import { readFileSync } from "node:fs";
const pkg = JSON.parse(readFileSync("./package.json", "utf8"));
process.stdout.write(`${pkg.name}\n${pkg.version}\n`);
EOF
)
  name=${fields%%$'\n'*}
  version_json=${fields#*$'\n'}
  version_json=${version_json%%$'\n'*}
  if npm view "$name@$version_json" version >/dev/null 2>&1; then
    echo "$name@$version_json is already published"
    return
  fi
  npm publish --access public --provenance
}

for dir in npm/platforms/*; do
  (cd "$dir" && publish)
done
(cd npm/types && publish)
(cd npm/cli && publish)
