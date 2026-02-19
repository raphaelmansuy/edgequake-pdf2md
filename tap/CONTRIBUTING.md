# Contributing

Thanks for helping maintain this Homebrew tap.

Guidelines

- Fork the repository and open a pull request against `main`.
- Make changes in a feature branch and include a clear commit message.
- Add or update tests where applicable (this repo includes a CI workflow
  that verifies upstream asset checksums).
- For updates to upstream assets, use `release/generate-sha256.sh` to compute
  checksums and include the pinned `sha256` in `Formula/pdfium.rb`.

Pull request process

1. Open a PR with a short description of the change.
2. Request review from a maintainer (see `CODEOWNERS`).
3. Ensure CI checks pass before merging.
