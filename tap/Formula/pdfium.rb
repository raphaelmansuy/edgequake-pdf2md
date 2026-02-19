class Pdfium < Formula
  desc "Prebuilt PDFium binaries (wrapper)"
  homepage "https://github.com/bblanchon/pdfium-binaries"
  # We use the chromium tag as the version identifier used by the upstream
  # pdfium-binaries releases. Update this when upstream publishes newer builds.
  version "chromium/7690"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-mac-arm64.tgz"
    else
      url "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-mac-x64.tgz"
      sha256 "04bc0a7696452c903f006812d877250cdbc462bfe785ec79bb41b7d145f5774f"
    end
    # Upstream assets change frequently; consider pinning the correct
    # `sha256` for production use (we compute these in tap/release/*).
    if Hardware::CPU.arm?
      sha256 "0617e90556273ebe484d5f4a9981f366e10b4874f997896a4d27fe87d3c71ecf"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-linux-arm64.tgz"
    else
      url "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7690/pdfium-linux-x64.tgz"
      sha256 "addd9f59beea62536ad45f0ca6d38fb6088ba20a7260443deb595852fb51eac3"
    end
    if Hardware::CPU.arm?
      sha256 "76ee55e238ccd226b55de569217dd93aba71ffdb5368d5f72b5c2b49fab6dee0"
    end
  end

  def install
    # Extract the prebuilt archive into the prefix and install the native
    # library where consumers expect it.
    lib.install Dir["**/libpdfium.*"]
  end

  def caveats
    <<~EOS
      This formula installs prebuilt PDFium native libraries from
      bblanchon/pdfium-binaries (version #{version}).

      - macOS users may need to set:
          export DYLD_LIBRARY_PATH="#{opt_lib}:$DYLD_LIBRARY_PATH"

      - Linux users may need to set:
          export LD_LIBRARY_PATH="#{opt_lib}:$LD_LIBRARY_PATH"

      Note: This is a community-maintained tap that fetches upstream binary
      releases. The upstream assets change often; consider pinning a specific
      upstream release or vendor the correct sha256 in this formula before
      using in production.
    EOS
  end

  test do
    # Basic smoke test: ensure the library file is installed somewhere in lib
    if OS.mac?
      assert_predicate lib/"libpdfium.dylib", :exist?
    elsif OS.linux?
      assert_predicate lib/"libpdfium.so", :exist?
    end
  end
end
