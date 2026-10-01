#!/bin/sh
# Set one version across the backend workspace, the frontend and the add-on,
# and open a CHANGELOG section for it. CI runs this for every release.
# Usage: bump-version.sh patch|minor|major|X.Y.Z
# CHANGELOG_NOTES may name a file of "- ..." lines for the new section (CI
# passes the commit subjects); without it the section gets an empty bullet.
set -eu

cd "$(dirname "$0")/.."

current=$(sed -n 's/^version: "\(.*\)"/\1/p' addon/config.yaml)
IFS=. read -r major minor patch <<EOF
$current
EOF

case "${1:-}" in
  patch) new="$major.$minor.$((patch + 1))" ;;
  minor) new="$major.$((minor + 1)).0" ;;
  major) new="$((major + 1)).0.0" ;;
  [0-9]*.[0-9]*.[0-9]*) new="$1" ;;
  *) echo "usage: $0 patch|minor|major|X.Y.Z" >&2; exit 1 ;;
esac

# Backend: the workspace version, plus the version pins on path dependencies.
sed -i "/^\[workspace.package\]/,/^\[/ s/^version = \".*\"/version = \"$new\"/" backend/Cargo.toml
sed -i -E "s/^(ergo-[a-z]+ = \{ version = )\"[^\"]*\"/\1\"$new\"/" backend/*/Cargo.toml
cargo update --manifest-path backend/Cargo.toml --workspace --quiet

# Frontend: package.json and package-lock.json.
(cd frontend && npm version "$new" --no-git-tag-version --allow-same-version >/dev/null)

# Add-on: the version HA shows, and the changelog section CI requires.
sed -i "s/^version: \".*\"/version: \"$new\"/" addon/config.yaml
if ! grep -q "^## $new\$" addon/CHANGELOG.md; then
  notes="${CHANGELOG_NOTES:-}"
  if [ -z "$notes" ]; then notes=$(mktemp) && echo "- " > "$notes"; fi
  awk -v v="$new" -v notes="$notes" '
    /^## / && !done { print "## " v "\n\n### Changed\n"; while ((getline l < notes) > 0) print l; print ""; done = 1 }
    { print }' addon/CHANGELOG.md > addon/CHANGELOG.md.new
  mv addon/CHANGELOG.md.new addon/CHANGELOG.md
fi

echo "$current -> $new"
