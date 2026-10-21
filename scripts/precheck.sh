#!/bin/bash
# precheck.sh — verify every version bump + changelog is consistent BEFORE a release.
# Run by scripts/release.sh automatically, or standalone: ./scripts/precheck.sh [version]
#
# Checks (each is a hard failure with a fix hint):
#   1. Cargo.toml version matches the target version
#   2. Cargo.lock package version matches (and is committed clean)
#   3. npm/package.json version + @modelsdesk platform pins match
#   4. Every npm/platforms/*/package.json version matches
#   5. CHANGELOG.md has a `## [<version>]` section (not just Unreleased)
#   6. Working tree is clean (cargo publish requires it)
#   7. The version is semver and greater than the last git tag
set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$DIR"

TARGET="${1:-}"
if [[ -z "$TARGET" ]]; then
  # Infer the intended version from the latest CHANGELOG entry.
  TARGET="$(grep -m1 -oE '## \[[0-9]+\.[0-9]+\.[0-9]+\]' CHANGELOG.md | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || true)"
  if [[ -z "$TARGET" ]]; then
    echo "error: pass a version: $0 <version> (or add a '## [x.y.z]' section to CHANGELOG.md)" >&2
    exit 1
  fi
  echo "precheck: inferred target version $TARGET from CHANGELOG.md"
fi

errors=0
fail() { echo "  ✗ $1" | sed 's/^/  /'; echo "    fix: $2"; errors=$((errors+1)); }
ok()   { echo "  ✓ $1"; }

semver_re='^[0-9]+\.[0-9]+\.[0-9]+$'
echo "— version format"
if [[ "$TARGET" =~ $semver_re ]]; then ok "semver: $TARGET"; else
  fail "'$TARGET' is not strict semver (MAJOR.MINOR.PATCH)" "use e.g. 0.3.0 (no pre-release/build suffixes yet)"
fi

echo "— git state"
if [[ -n "$(git status --porcelain)" ]]; then
  fail "working tree has uncommitted changes" "commit or stash first (cargo publish needs a clean tree)"
else
  ok "clean tree"
fi
LAST_TAG="$(git tag --sort=-v:refname | grep -E '^v[0-9]' | head -1 || true)"
if [[ -n "$LAST_TAG" ]]; then
  last="${LAST_TAG#v}"
  newer="$(printf '%s\n%s\n' "$last" "$TARGET" | sort -V | tail -1)"
  if [[ "$newer" == "$TARGET" && "$TARGET" != "$last" ]]; then
    ok "$TARGET > last tag $LAST_TAG"
  else
    fail "$TARGET is not greater than the last tag $LAST_TAG" "bump to a higher semver (tags are immutable)"
  fi
else
  ok "no previous tags"
fi

echo "— Cargo.toml"
cargo_ver="$(grep -m1 '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/')"
if [[ "$cargo_ver" == "$TARGET" ]]; then ok "Cargo.toml version = $cargo_ver"; else
  fail "Cargo.toml is $cargo_ver, expected $TARGET" "sed -i '' 's/^version = \".*\"/version = \"$TARGET\"/' Cargo.toml && cargo check"
fi

echo "— Cargo.lock"
lock_ver="$(grep -A1 '^name = "modelsdesk"' Cargo.lock | grep -m1 version | sed 's/version = "\(.*\)"/\1/')"
if [[ "$lock_ver" == "$TARGET" ]]; then ok "Cargo.lock modelsdesk = $lock_ver"; else
  fail "Cargo.lock has modelsdesk $lock_ver, expected $TARGET" "run: cargo update -p modelsdesk (or cargo check) and commit Cargo.lock"
fi

echo "— npm manifests"
root_ver="$(jq -r .version npm/package.json)"
if [[ "$root_ver" == "$TARGET" ]]; then ok "npm/package.json = $root_ver"; else
  fail "npm/package.json is $root_ver, expected $TARGET" "jq '.version = \"$TARGET\"' npm/package.json"
fi
# Every optionalDependency pin on @modelsdesk/* must match the root version exactly.
while IFS='=' read -r dep pin; do
  if [[ "$pin" == "$TARGET" ]]; then ok "pin $dep = $pin"; else
    fail "npm/package.json pins $dep at $pin, expected $TARGET" \
      "jq --arg v $TARGET '.optionalDependencies[\"$dep\"] = \$v' npm/package.json (release.sh does this; was this edited by hand?)"
  fi
done < <(jq -r '.optionalDependencies | to_entries[] | "\(.key)=\(.value)"' npm/package.json)
for pkg in npm/platforms/*/package.json; do
  plat_ver="$(jq -r .version "$pkg")"
  if [[ "$plat_ver" == "$TARGET" ]]; then ok "$pkg = $plat_ver"; else
    fail "$pkg is $plat_ver, expected $TARGET" "jq '.version = \"$TARGET\"' $pkg"
  fi
done

echo "— changelog"
if grep -q "^## \[$TARGET\]" CHANGELOG.md; then ok "CHANGELOG.md has a [$TARGET] section"; else
  fail "CHANGELOG.md has no '## [$TARGET]' section" "add '## [$TARGET] - $(date +%Y-%m-%d)' with Added/Fixed entries, and move Unreleased items under it"
fi
if grep -B2 "^## \[$TARGET\]" CHANGELOG.md | grep -q '^### '; then :; fi
if grep -A3 "^## \[$TARGET\]" CHANGELOG.md | grep -q '^### \(Added\|Changed\|Deprecated\|Removed\|Fixed\|Security\)'; then
  ok "[$TARGET] section has Keep-a-Changelog categories"
else
  fail "[$TARGET] changelog section has no ### Added/Changed/Fixed heading" "use at least one of: Added, Changed, Deprecated, Removed, Fixed, Security"
fi
if ! grep -q "^## \[$TARGET\] - [0-9]\{4\}-[0-9]\{2\}-[0-9]\{2\}" CHANGELOG.md; then
  fail "[$TARGET] heading has no date" "use '## [$TARGET] - YYYY-MM-DD'"
else
  ok "[$TARGET] heading is dated"
fi

echo ""
if [[ "$errors" -gt 0 ]]; then
  echo "✗ precheck failed: $errors problem(s) — release aborted"
  exit 1
fi
echo "✓ precheck passed: all bumps + changelog consistent for $TARGET"
