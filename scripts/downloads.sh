#!/bin/sh
# Brink download counts from GitHub (the source of truth: every DMG download, from any link).
#   scripts/downloads.sh
# DMG = new installs. latest.json = update checks from running copies. .tar.gz = updates installed.
set -e
GH_TOKEN=$(gh auth token --user MuhammadaliMS) gh api --paginate repos/MuhammadaliMS/brink-releases/releases --jq '
  .[] | "\(.tag_name)  \(.published_at[:10])",
  (.assets[] | "  \(.download_count | tostring | (" " * (6 - length)) + .)  \(.name)")'
echo
GH_TOKEN=$(gh auth token --user MuhammadaliMS) gh api --paginate repos/MuhammadaliMS/brink-releases/releases --jq '
  [.[].assets[] | select(.name | endswith(".dmg")) | .download_count] | add // 0 | "DMG downloads, all releases: \(.)"'
