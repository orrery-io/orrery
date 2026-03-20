#!/usr/bin/env bash
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_DIR"

if [[ $# -eq 0 ]]; then
  # Auto-increment patch from latest git tag
  LATEST=$(git tag --list 'v*' --sort=-version:refname | head -1)
  if [[ -z "$LATEST" ]]; then
    echo "Error: no existing version tags found; provide a version explicitly."
    echo "Usage: $0 <version>"
    exit 1
  fi
  BASE="${LATEST#v}"
  MAJOR="${BASE%%.*}"; REST="${BASE#*.}"
  MINOR="${REST%%.*}"; PATCH="${REST#*.}"
  VERSION="${MAJOR}.${MINOR}.$((PATCH + 1))"
  echo "==> Auto-incrementing from $LATEST → v$VERSION"
elif [[ $# -eq 1 ]]; then
  VERSION="$1"
else
  echo "Usage: $0 [version]"
  echo "Example: $0 0.2.0  (or omit to auto-increment patch)"
  exit 1
fi

TAG="v${VERSION}"

# Validate semver format
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Error: version must be in X.Y.Z format (got: $VERSION)"
  exit 1
fi

CARGO_FILES=(
  "Cargo.toml"
  "crates/orrery-types/Cargo.toml"
  "crates/orrery-client/Cargo.toml"
  "crates/orrery-worker/Cargo.toml"
)

echo "==> Current versions"
grep -h "^version\|orrery-types\|orrery-client\|orrery-worker" "${CARGO_FILES[@]}" | grep -v "^#" || true

echo ""
echo "==> Bumping all versions to $VERSION"

CURRENT_VERSIONS=$(grep -h '^version = ' "${CARGO_FILES[@]}" | sort -u | sed 's/version = "\(.*\)"/\1/')

for OLD in $CURRENT_VERSIONS; do
  if [[ "$OLD" != "$VERSION" ]]; then
    sed -i '' "s/version = \"$OLD\"/version = \"$VERSION\"/g" "${CARGO_FILES[@]}"
  fi
done

echo "==> Refreshing Cargo.lock"
cargo check -p orrery-types -p orrery-client -p orrery-worker

TS_PKG="sdks/typescript/package.json"
if [[ -f "$TS_PKG" ]]; then
  echo "==> Bumping TypeScript SDK to $VERSION"
  sed -i '' "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" "$TS_PKG"
fi

echo "==> Committing"
git add Cargo.toml Cargo.lock \
  crates/orrery-types/Cargo.toml \
  crates/orrery-client/Cargo.toml \
  crates/orrery-worker/Cargo.toml
if [[ -f "$TS_PKG" ]]; then
  git add "$TS_PKG"
fi
git commit -m "chore: bump to $TAG"

echo "==> Tagging and pushing"
git tag "$TAG"
git push origin main
git push origin "$TAG"

echo ""
echo "Done! CI will publish crates and build Docker image for $TAG."
