#!/usr/bin/env bash
# One-shot publish. Run AFTER `gh auth login`.
#
#   ./publish.sh YOUR_GITHUB_USERNAME
#
# Swaps the repo placeholders, creates the public repo, pushes main.
set -euo pipefail

OWNER="${1:?usage: ./publish.sh YOUR_GITHUB_USERNAME}"
SLUG="bkash-desktop"

command -v gh >/dev/null || { echo "gh not found: https://cli.github.com"; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "not logged in — run: gh auth login"; exit 1; }

echo "→ swapping placeholders to $OWNER/$SLUG"
for f in README.md web/index.html web/robots.txt web/sitemap.xml .github/workflows/ci.yml; do
  [ -f "$f" ] || continue
  sed -i "s#OWNER/$SLUG#$OWNER/$SLUG#g; s#https://$SLUG\.example#https://$OWNER.github.io/$SLUG#g; s#$SLUG\.example#$OWNER.github.io/$SLUG#g" "$f"
done

echo "→ verifying the build before publishing"
cargo test --quiet >/dev/null && echo "  tests pass"

echo "→ creating the repo"
gh repo create "$SLUG" --public --source=. --remote=origin --description \
  "Native Rust reimplementation of the bKash Android client — runs on your PC, no emulator" \
  --push

echo "→ done: https://github.com/$OWNER/$SLUG"
