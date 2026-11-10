class Oxidetime < Formula
  desc "Local macOS app usage tracker with a built-in web dashboard"
  homepage "https://github.com/jimicat/OxideTime"
  url "https://github.com/jimicat/OxideTime/archive/a3607dd8c63c5b590c34c8e1cd47520ce3f7a9bf.tar.gz"
  version "0.1.1"
  sha256 "194c2018755aba330037f4f7e2271a9abd784196a62ce9aca193e24503c6b7a5"
  license "MIT"

  depends_on "rust" => :build
  depends_on :macos

  def install
    system "cargo", "install", *std_cargo_args
  end

  service do
    run [opt_bin/"oxidetime"]
    keep_alive true
    log_path var/"log/oxidetime.log"
    error_log_path var/"log/oxidetime.log"
  end

  test do
    assert_match "OxideTime", shell_output("#{bin}/oxidetime --help")
  end
end
