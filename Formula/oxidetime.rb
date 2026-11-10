class Oxidetime < Formula
  desc "Local macOS app usage tracker with a built-in web dashboard"
  homepage "https://github.com/jimicat/OxideTime"
  url "https://github.com/jimicat/OxideTime/archive/fc380d721fd51d682dc425446f123a3639c9f2d1.tar.gz"
  version "0.1.0"
  sha256 "2b66cac6b89a030024dbc987ff2e1ca383cae7521ff1b75f1a89d91041f8db29"
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
