class Oxidetime < Formula
  desc "Local macOS app usage tracker with a built-in web dashboard"
  homepage "https://github.com/jimicat/OxideTime"
  license "MIT"

  depends_on :macos

  on_arm do
    url "https://github.com/jimicat/OxideTime/releases/download/v0.1.2/oxidetime-0.1.2-aarch64-apple-darwin.tar.gz"
    sha256 "123c4fa904a9088771797c8d3dc9ce53d772e47de9322952503eaadea9887af5"
  end

  on_intel do
    url "https://github.com/jimicat/OxideTime/releases/download/v0.1.2/oxidetime-0.1.2-x86_64-apple-darwin.tar.gz"
    sha256 "e5e86744f1465566d97f9cdebd8095b4c2fc73a29807e0067ac189044c5c2940"
  end

  def install
    bin.install "oxidetime"
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
