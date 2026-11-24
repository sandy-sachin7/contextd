# Homebrew formula for contextd
# Install from source: brew install --build-from-source ./homebrew/contextd.rb
# Or via tap: brew install sandy-sachin7/tap/contextd

class Contextd < Formula
  desc "A local-first semantic context daemon for AI agents"
  homepage "https://github.com/sandy-sachin7/contextd"
  license "MIT"
  head "https://github.com/sandy-sachin7/contextd.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--locked", *std_cargo_args
    generate_completions_from_executable(bin/"contextd", "completions")
  end

  def caveats
    <<~EOS
      To download the default embedding model:
        contextd setup

      To start the contextd daemon:
        contextd daemon

      To run as an MCP server:
        contextd mcp

      To configure your AI tools automatically:
        contextd connect --all

      User configuration location:
        ~/.config/contextd/contextd.toml
    EOS
  end

  test do
    assert_match "contextd", shell_output("#{bin}/contextd --version")
  end
end
