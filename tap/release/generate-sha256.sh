#!/usr/bin/env bash
set -euo pipefail

USAGE="Usage: $0 <asset-url> [--apply]

Downloads <asset-url>, prints the sha256 and optionally updates Formula/pdfium.rb
replacing 'sha256 :no_check' with the computed sha256. Use --apply to modify file.
"

if [ $# -lt 1 ]; then
  echo "$USAGE"; exit 2
fi

URL="$1"
APPLY=false
if [ "${2:-}" = "--apply" ]; then APPLY=true; fi

tmp=$(mktemp -d)
asset="$tmp/asset.tgz"
echo "Downloading $URL to $asset"
curl -sSL "$URL" -o "$asset"
sha256=$(sha256sum "$asset" | awk '{print $1}')
echo "sha256: $sha256"

if [ "$APPLY" = true ]; then
  formula="Formula/pdfium.rb"
  if [ ! -f "$formula" ]; then echo "Formula not found: $formula" >&2; exit 3; fi
  # Replace first occurrence of sha256 :no_check with sha256 "<value>"
  awk -v s="$sha256" 'BEGIN{replaced=0} {
    if(!replaced && $0 ~ /sha256[[:space:]]+:no_check/) {
      print "    sha256 \"" s "\""
      replaced=1
    } else print $0
  }' "$formula" > "$formula.tmp" && mv "$formula.tmp" "$formula"
  echo "Applied sha256 to $formula"
fi

rm -rf "$tmp"
