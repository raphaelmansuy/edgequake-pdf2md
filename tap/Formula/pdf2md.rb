# frozen_string_literal: true

# Formula for pdf2md — PDF to Markdown converter using Vision LLMs.
# Part of the raphaelmansuy/homebrew-pdfium-tap tap.
#
# This formula builds edgequake-pdf2md from source with the `bundled` feature,
# which embeds a prebuilt pdfium native library directly into the binary.
# The pdfium library is fetched as a `resource` block so Homebrew's sandbox
# does not need network access during the cargo build step.
#
# Pdfium version: chromium/7690 (bblanchon/pdfium-binaries)
class Pdf2md < Formula
  desc "Convert PDF documents to Markdown using Vision LLMs (self-contained)"
  homepage "https://github.com/raphaelmansuy/edgequake-pdf2md"
  url "https://github.com/raphaelmansuy/edgequake-pdf2md/archive/refs/tags/v0.4.2.tar.gz"
  sha256 "883b3e3a71995449e0cd970f8664ff18ebf428f7fd5ce0d8592c93a5555251ad"
  license "MIT"
  head "https://github.com/raphaelmansuy/edgequake-pdf2md.git", branch: "main"

  depends_on "rust" => :build

  # Pre-download the correct pdfium prebuilt library for each platform so that
  # build.rs can copy it in without reaching out to the network (Homebrew
  # sandboxes network calls during `install`).
  #
  # SHA256 values correspond to pdfium-binaries chromium/7690 assets.

  on_macos do
    on_arm do
      resource "pdfium" do
        url "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-mac-arm64.tgz"
        sha256 "0617e90556273ebe484d5f4a9981f366e10b4874f997896a4d27fe87d3c71ecf"
      end
    end

    on_intel do
      resource "pdfium" do
        url "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-mac-x64.tgz"
        sha256 "04bc0a7696452c903f006812d877250cdbc462bfe785ec79bb41b7d145f5774f"
      end
    end
  end

  on_linux do
    on_arm do
      resource "pdfium" do
        url "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-linux-arm64.tgz"
        sha256 "76ee55e238ccd226b55de569217dd93aba71ffdb5368d5f72b5c2b49fab6dee0"
      end
    end

    on_intel do
      resource "pdfium" do
        url "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-linux-x64.tgz"
        sha256 "addd9f59beea62536ad45f0ca6d38fb6088ba20a7260443deb595852fb51eac3"
      end
    end
  end

  def install
    # Stage the pdfium prebuilt library into buildpath so cargo build.rs can
    # find it via PDFIUM_BUNDLE_LIB (avoids the network sandbox restriction).
    pdfium_lib_dir = buildpath/"pdfium-bundle"
    pdfium_lib_dir.mkpath

    resource("pdfium").stage do
      if OS.mac?
        cp "lib/libpdfium.dylib", pdfium_lib_dir
        ENV["PDFIUM_BUNDLE_LIB"] = (pdfium_lib_dir/"libpdfium.dylib").to_s
      else
        cp "lib/libpdfium.so", pdfium_lib_dir
        ENV["PDFIUM_BUNDLE_LIB"] = (pdfium_lib_dir/"libpdfium.so").to_s
      end
    end

    # Build the self-contained release binary.
    # `std_cargo_args` expands to: --locked --root=prefix --bin=pdf2md
    system "cargo", "install", *std_cargo_args
  end

  test do
    # Verify the binary runs and reports the correct version.
    assert_match "pdf2md #{version}", shell_output("#{bin}/pdf2md --version 2>&1")
  end
end
