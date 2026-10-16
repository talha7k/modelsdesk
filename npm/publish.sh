#!/bin/bash
# Publish modeldesk to npm: platform package(s) first, then the root.
# Usage: ./npm/publish.sh            (uses working tree binaries)
# Auth: run under `varlock run` with NPM_TOKEN in env, and a 0600 .npmrc
# carrying `//registry.npmjs.org/:_authToken=${NPM_TOKEN}` in the npm/ dir.
set -euo pipefail
cd "$(dirname "$0")"

VERSION="$(node -p "require('./package.json').version")"
echo "modeldesk npm publish v$VERSION"

# 1. Stage skills into the tarball (source of truth: repo root skills/)
rm -rf ./skills
cp -r ../skills ./skills

# 2. Stage the release binary into each platform package
cargo build --release --manifest-path ../Cargo.toml
mkdir -p ./platforms/darwin-arm64/bin
cp ../target/release/modeldesk ./platforms/darwin-arm64/bin/modeldesk

# 3. Publish platform packages first (root depends on them)
for plat in platforms/*/; do
  echo "--- publishing $plat"
  npm publish --access public "$plat"
done

# 4. Publish the root wrapper
echo "--- publishing root"
npm publish --access public .

# 5. Clean staged artifacts (binaries + skills copy never committed)
rm -rf ./skills ./platforms/*/bin/modeldesk
echo "done: modeldesk@$VERSION"
