# Homebrew formula for contextd
# Install: brew install sandy-sachin7/tap/contextd
# Or local: brew install --build-from-source ./homebrew/contextd.rb

class Contextd < Formula
  desc "A local-first semantic context daemon for AI agents"
  homepage "https://github.com/sandy-sachin7/contextd"
  version "0.1.1"
  license "MIT"

  on_macos do
    on_intel do
      url "https://github.com/sandy-sachin7/contextd/releases/download/v#{version}/contextd-macos-x86_64"
      sha256 "PLACEHOLDER_SHA256_MACOS_X86_64"
    end

    on_arm do
      url "https://github.com/sandy-sachin7/contextd/releases/download/v#{version}/contextd-macos-aarch64"
      sha256 "PLACEHOLDER_SHA256_MACOS_AARCH64"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/sandy-sachin7/contextd/releases/download/v#{version}/contextd-linux-x86_64"
      sha256 "PLACEHOLDER_SHA256_LINUX_X86_64"
    end

    on_arm do
      url "https://github.com/sandy-sachin7/contextd/releases/download/v#{version}/contextd-linux-aarch64"
      sha256 "PLACEHOLDER_SHA256_LINUX_AARCH64"
    end
  end

  def install
    binary_name = if OS.mac?
      Hardware::CPU.arm? ? "contextd-macos-aarch64" : "contextd-macos-x86_64"
    else
      Hardware::CPU.arm? ? "contextd-linux-aarch64" : "contextd-linux-x86_64"
    end

    bin.install Dir["*"].first => "contextd"
  end

  def caveats
    <<~EOS
      To start contextd daemon:
        contextd daemon

      To run as MCP server:
        contextd mcp

      Configuration file location:
        ~/.config/contextd/contextd.toml

      For more information:
        https://github.com/sandy-sachin7/contextd
    EOS
  end

  test do
    assert_match "contextd", shell_output("#{bin}/contextd --version")
  end
end
