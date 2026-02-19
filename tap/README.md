# homebrew-pdfium-tap

Homebrew tap providing a community-maintained `pdfium` formula that wraps
prebuilt binaries from `bblanchon/pdfium-binaries`.

This repository is a minimal tap intended to make it easier to install the
`libpdfium` native libraries on macOS and Linux. It fetches upstream
precompiled artifacts and installs the native `libpdfium` files into Homebrew's
`lib` prefix.

IMPORTANT: Upstream binary releases change often. This tap uses `sha256 :no_check`
by default so it's convenient for testing, but you should pin a specific
upstream release and add the proper `sha256` in the formula before using in
production.

Usage

```bash
# Tap the repository (replace USER with the GitHub owner)
brew tap USER/homebrew-pdfium-tap

# Install pdfium
brew install USER/pdfium

# After installation, set the dynamic library path if necessary
# macOS
export DYLD_LIBRARY_PATH="$(brew --prefix USER/pdfium)/lib:$DYLD_LIBRARY_PATH"

# Linux
export LD_LIBRARY_PATH="$(brew --prefix USER/pdfium)/lib:$LD_LIBRARY_PATH"
```

Publishing

1. Create a new GitHub repository named `homebrew-pdfium-tap` under your
   account (e.g. `github.com/USER/homebrew-pdfium-tap`).
2. Push this tap's files (Formula/pdfium.rb, README.md) to the repo's `main`
   branch.
3. Users can then install via `brew tap USER/homebrew-pdfium-tap`.

Security & maintenance notes

- Consider pinning the upstream release and adding the `sha256` to the
  formula.
- Consider adding CI actions to verify the formula builds and the asset
  URLs are reachable.

If you want I can prepare a GitHub Actions workflow to automate release checks
and help generate proper `sha256` values for each new upstream release.

Automated release helpers

This tap includes helpers to verify asset URLs and compute sha256 values:

- `release/check_assets.sh Formula/pdfium.rb` — downloads URLs referenced by the formula and prints sha256 sums.
- `release/generate-sha256.sh <asset-url> [--apply]` — download an asset and either print the sha256 or patch `Formula/pdfium.rb` replacing the first `sha256 :no_check` entry with the computed sha256.

Example (compute only):

```bash
./release/generate-sha256.sh https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-mac-x64.tgz
```

Example (apply to formula):

```bash
./release/generate-sha256.sh https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-mac-x64.tgz --apply
git add Formula/pdfium.rb && git commit -m "chore(pdfium): pin sha256 for chromium/7690" && git push origin main
```
