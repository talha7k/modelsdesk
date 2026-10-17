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

# 1. Bump version in all manifests
echo "— bumping version"
sed -i.bak "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml
rm -f Cargo.toml.bak

for pkg in npm/package.json npm/platforms/darwin-arm64/package.json; do
  jq -M --arg v "$VERSION" '.version = $v' "$pkg" > "$pkg.tmp" && mv "$pkg.tmp" "$pkg"
done

# 2. Commit + tag
echo "— committing"
git add Cargo.toml npm/package.json npm/platforms/darwin-arm64/package.json
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
