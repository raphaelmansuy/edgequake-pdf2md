# homebrew-pdfium-tap

Homebrew tap providing a community-maintained `pdfium` formula that wraps
prebuilt binaries from `bblanchon/pdfium-binaries`.

Repository: https://github.com/raphaelmansuy/homebrew-pdfium-tap

Status

- Published under `raphaelmansuy/homebrew-pdfium-tap` on GitHub.
- The `Formula/pdfium.rb` in this repo pins sha256 values for known upstream
  assets and includes CI helpers to verify asset availability.

Quick install

```bash
# Tap the repository
brew tap raphaelmansuy/homebrew-pdfium-tap

# Install the pdfium formula
brew install pdfium
```

Or install directly from the raw formula URL without tapping:

```bash
brew install https://raw.githubusercontent.com/raphaelmansuy/homebrew-pdfium-tap/main/Formula/pdfium.rb
```

After install, the native libraries are installed into the formula prefix. To
add the library path to your environment:

```bash
# macOS
export DYLD_LIBRARY_PATH="$(brew --prefix pdfium)/lib:$DYLD_LIBRARY_PATH"

# Linux
export LD_LIBRARY_PATH="$(brew --prefix pdfium)/lib:$LD_LIBRARY_PATH"
```

Notes

- The formula currently pins sha256 values for the upstream assets — this
  protects installations from upstream changes. When upstream releases are
  updated the release helper scripts can be used to recompute and patch the
  formula.
- CI: this repository includes a `verify-assets.yml` workflow that checks the
  availability and checksums of upstream assets on push or dispatch.

Publishing & maintenance

If you want to update the formula for a new upstream release:

1. Run `release/generate-sha256.sh <asset-url>` to compute the sha256 for each asset.
2. Use `--apply` to patch `Formula/pdfium.rb` and commit the change.
3. Push a branch/PR and merge when checks pass.

Helpers

- `release/check_assets.sh Formula/pdfium.rb` — downloads URLs referenced by the formula and prints sha256 sums.
- `release/generate-sha256.sh <asset-url> [--apply]` — download an asset and either print the sha256 or patch `Formula/pdfium.rb` replacing the first `sha256 :no_check` entry with the computed sha256.

Contributing

Open an issue or a PR against this repository for feature requests, bugs, or
to update upstream asset pins.

License: same as the parent project.
