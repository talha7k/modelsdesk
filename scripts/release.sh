#!/bin/bash
# Release modeldesk: bump version, tag, push, publish to crates.io + npm.
# Usage: ./scripts/release.sh <version>   (e.g. ./scripts/release.sh 0.2.0)
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <version>"
  exit 1
fi

VERSION="$1"
DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$DIR"

echo "=== Releasing modelsdesk@$VERSION ==="

# 0. Changelog guard: the version must have an entry in CHANGELOG.md
if ! grep -q "## \[$VERSION\]" CHANGELOG.md; then
  echo "error: CHANGELOG.md has no entry for [$VERSION]" >&2
  echo "add a '## [$VERSION] - YYYY-MM-DD' section before releasing" >&2
  exit 1
fi

# 1. Bump version in all manifests
echo "— bumping version"
sed -i.bak "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml
rm -f Cargo.toml.bak

for pkg in npm/package.json npm/platforms/darwin-arm64/package.json; do
  jq -M --arg v "$VERSION" '.version = $v' "$pkg" > "$pkg.tmp" && mv "$pkg.tmp" "$pkg"
done
# Keep the platform pin in lockstep with the root version.
jq -M --arg v "$VERSION" \
  '.optionalDependencies["@modelsdesk/darwin-arm64"] = $v' \
  npm/package.json > npm/package.json.tmp && mv npm/package.json.tmp npm/package.json

# 2. Commit + tag
echo "— committing"
git add Cargo.toml npm/package.json npm/platforms/darwin-arm64/package.json CHANGELOG.md
git commit -m "chore(release): v$VERSION" --quiet
git tag "v$VERSION"
echo "— pushing"
git push origin main "v$VERSION"

# 3. Publish crate to crates.io
echo "— publishing crate"
varlock run -- cargo publish
echo "  ✓ crates.io: modelsdesk@$VERSION"

# 4. Publish npm packages via worktree (keeps staged artifacts out of main tree)
echo "— publishing npm"
WT="$DIR/.worktrees/publish-v$VERSION"
git worktree remove --force "$WT" 2>/dev/null || true
git worktree add "$WT" "v$VERSION"
printf '//registry.npmjs.org/:_authToken=${NPM_TOKEN}\n' > "$WT/npm/.npmrc"
chmod 600 "$WT/npm/.npmrc"
varlock run -- bash -c "cd '$WT' && ./npm/publish.sh"
git worktree remove --force "$WT"
echo "  ✓ npm: modelsdesk@$VERSION"

# 5. Verify
echo "— verifying"
npm view modelsdesk dist-tags --prefer-online 2>/dev/null | grep "$VERSION" && echo "  ✓ npm resolved"
cargo search modelsdesk --limit 1 2>/dev/null | grep "$VERSION" && echo "  ✓ crates.io resolved"

echo ""
echo "✅ modelsdesk@$VERSION published to npm + crates.io"
