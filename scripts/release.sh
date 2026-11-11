#!/usr/bin/env bash
# Build, sign, notarize and publish a Brink release (maintainer only), in one command:
#
#   scripts/release.sh 1.0.1 "What changed, one line"        # build + publish
#   scripts/release.sh 1.0.1 "What changed" --dry-run        # build only, no upload
#
# Needs (in the environment): APPLE_SIGNING_IDENTITY (Tauri signs with it), APPLE_ID, APPLE_PASSWORD,
# APPLE_TEAM_ID. The updater key is ~/.tauri/brink.key, its password ~/.tauri/brink.key.password.
set -euo pipefail

VERSION="${1:?usage: release.sh <version> \"<notes>\" [--dry-run]}"
NOTES="${2:-Bug fixes and improvements.}"
DRY="${3:-}"
REPO="MuhammadaliMS/brink-releases"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAURI="$ROOT/src-tauri"
TARGET="universal-apple-darwin"
BUNDLE="$TAURI/target/$TARGET/release/bundle"
OUT="$ROOT/dist/v$VERSION"

cd "$ROOT"
# shellcheck disable=SC1090
[ -f ~/.zshrc ] && source <(grep -E '^export (APPLE_[A-Z_]+)=' ~/.zshrc)
export TAURI_SIGNING_PRIVATE_KEY="${BRINK_UPDATER_KEY:-$(cat ~/.tauri/brink.key)}"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${BRINK_UPDATER_KEY_PASSWORD:-$(cat ~/.tauri/brink.key.password)}"
for v in APPLE_SIGNING_IDENTITY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID; do
  [ -n "${!v:-}" ] || { echo "✗ $v is not set (see ~/.zshrc)"; exit 1; }
done

echo "▸ Brink $VERSION"

# 1. Bump the version everywhere it lives.
python3 - "$VERSION" <<'PY'
import json, re, sys
v = sys.argv[1]
for path in ["src-tauri/tauri.conf.json", "package.json"]:
    data = json.load(open(path))
    data["version"] = v
    open(path, "w").write(json.dumps(data, indent=2, ensure_ascii=False) + "\n")
p = "src-tauri/Cargo.toml"
s = open(p).read()
s = re.sub(r'(?m)^version = "[^"]+"', f'version = "{v}"', s, count=1)
open(p, "w").write(s)
PY

# 2. Tests, then a universal (Apple silicon + Intel) build. Tauri code-signs with the
#    Developer ID, notarizes, staples, builds the DMG and signs the updater tarball.
cargo test --manifest-path "$TAURI/Cargo.toml" --quiet
rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
pnpm tauri build --target "$TARGET"

APP_TGZ="$BUNDLE/macos/Brink.app.tar.gz"
DMG_SRC="$(ls "$BUNDLE"/dmg/Brink_"$VERSION"_*.dmg | head -1)"
[ -f "$APP_TGZ" ] && [ -f "$APP_TGZ.sig" ] && [ -f "$DMG_SRC" ] || { echo "✗ bundle artifacts missing"; exit 1; }

# 3. Notarize + staple the DMG itself so it opens without a Gatekeeper warning offline.
echo "▸ notarizing DMG"
xcrun notarytool submit "$DMG_SRC" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" --wait
xcrun stapler staple "$DMG_SRC"
spctl -a -vv -t install "$DMG_SRC" || true

# 4. Collect artifacts under stable names. Brink.dmg is what the website links to.
rm -rf "$OUT" && mkdir -p "$OUT"
cp "$DMG_SRC" "$OUT/Brink_${VERSION}_universal.dmg"
cp "$DMG_SRC" "$OUT/Brink.dmg"
cp "$APP_TGZ" "$OUT/Brink.app.tar.gz"
cp "$APP_TGZ.sig" "$OUT/Brink.app.tar.gz.sig"

SIG="$(cat "$APP_TGZ.sig")"
URL="https://github.com/$REPO/releases/download/v$VERSION/Brink.app.tar.gz"
python3 - "$VERSION" "$NOTES" "$SIG" "$URL" > "$OUT/latest.json" <<'PY'
import json, sys, datetime
v, notes, sig, url = sys.argv[1:]
plat = {"signature": sig, "url": url}
print(json.dumps({
    "version": v,
    "notes": notes,
    "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "platforms": {"darwin-aarch64": plat, "darwin-x86_64": plat},
}, indent=2))
PY
echo "▸ artifacts in $OUT"
ls -lh "$OUT"

if [ "$DRY" = "--dry-run" ]; then
  echo "✓ dry run: nothing uploaded"
  exit 0
fi

# 5. Publish to the public releases repo (the updater fetches latest.json from it
#    unauthenticated, so it has to be public). gh's active account may be a different
#    user; use the MuhammadaliMS token explicitly.
export GH_TOKEN="${GH_TOKEN:-$(gh auth token --user MuhammadaliMS)}"
gh repo view "$REPO" >/dev/null 2>&1 || gh repo create "$REPO" --public --description "Brink releases (macOS)"
# A release needs a commit to tag; give a brand-new repo its README first.
if ! gh api "repos/$REPO/commits" --silent >/dev/null 2>&1; then
  printf '# Brink for macOS\n\nRelease builds and the auto-update feed for Brink. https://zerowing.in/brink/\n' > "$OUT/README.md"
  gh api -X PUT "repos/$REPO/contents/README.md" -f message="Add README" -f content="$(base64 -i "$OUT/README.md")" --silent
fi
gh release create "v$VERSION" --repo "$REPO" --title "Brink $VERSION" --notes "$NOTES" \
  "$OUT/Brink_${VERSION}_universal.dmg" "$OUT/Brink.dmg" "$OUT/Brink.app.tar.gz" "$OUT/Brink.app.tar.gz.sig" "$OUT/latest.json"

# 6. Verify what installed copies will see.
sleep 3
curl -fsSL "https://github.com/$REPO/releases/latest/download/latest.json" | python3 -m json.tool
echo "✓ Brink $VERSION released. Commit the version bump:"
echo "  git commit -am \"chore: release $VERSION\" && git tag v$VERSION"
