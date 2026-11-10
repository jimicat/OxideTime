class Oxidetime < Formula
  desc "Local macOS app usage tracker with a built-in web dashboard"
  homepage "https://github.com/jimicat/OxideTime"
  url "https://github.com/jimicat/OxideTime/archive/6f297209ca4d9c6f6bca4a849f1e1613b3dfb033.tar.gz"
  version "0.1.0"
  sha256 "ff7b3ca0a2fb040e48d99bfa927a2dad0427031e8e4b03acdc34df04ee8f7062"
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
