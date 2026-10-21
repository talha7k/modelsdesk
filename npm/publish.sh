#!/bin/bash
# Publish modeldesk to npm: platform package(s) first, then the root.
# Usage: ./npm/publish.sh            (uses working tree binaries)
# Auth: run under `varlock run` with NPM_TOKEN in env, and a 0600 .npmrc
# carrying `//registry.npmjs.org/:_authToken=${NPM_TOKEN}` in the npm/ dir.
set -euo pipefail
cd "$(dirname "$0")"

VERSION="$(node -p "require('./package.json').version")"
echo "modelsdesk npm publish v$VERSION"

# 1. Stage skills + CHANGELOG into the tarball (source of truth: repo root)
rm -rf ./skills
cp -r ../skills ./skills
cp ../CHANGELOG.md ./CHANGELOG.md 2>/dev/null || true

# 2. Stage the release binary into each platform package
# (cd to the repo root so target/ lands where the cp expects it)
(cd .. && cargo build --release)
mkdir -p ./platforms/darwin-arm64/bin
cp ../target/release/modelsd ./platforms/darwin-arm64/bin/modelsd

# 3. Publish platform packages first (root depends on them)
for plat in platforms/*/; do
  echo "--- publishing $plat"
  npm publish --access public "$plat"
done

# 4. Publish the root wrapper
echo "--- publishing root"
npm publish --access public .

# 5. Clean staged artifacts (binaries + skills copy never committed)
rm -rf ./skills ./CHANGELOG.md ./platforms/*/bin/modelsd
echo "done: modelsdesk@$VERSION"
