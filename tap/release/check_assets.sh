#!/usr/bin/env bash
set -euo pipefail

FORMULA_FILE="${1:-Formula/pdfium.rb}"
if [ ! -f "$FORMULA_FILE" ]; then
  echo "Formula not found: $FORMULA_FILE" >&2
  exit 2
fi

echo "Reading formula: $FORMULA_FILE"

# Extract all URLs in the formula
URLS=$(grep -oE "https?://[^\"']+" "$FORMULA_FILE" | sort -u)
if [ -z "$URLS" ]; then
  echo "No URLs found in formula"
  exit 0
fi

echo "Found URLs:"; echo "$URLS"

echo
for url in $URLS; do
  echo "---"
  echo "Checking: $url"
  # Test HEAD first
  if curl -sSfI "$url" >/dev/null; then
    echo "URL reachable"
  else
    echo "Failed to reach URL: $url" >&2
    continue
  fi

  # Download to temp and compute sha256
  tmp=$(mktemp -d)
  fname="$tmp/asset.tgz"
  echo "Downloading asset to $fname (this may take a while)"
  curl -sSL "$url" -o "$fname"
  echo "Computing sha256..."
  sha256=$(sha256sum "$fname" | awk '{print $1}')
  echo "sha256: $sha256"
  rm -rf "$tmp"
done

echo "Done"
