#!/bin/sh
# Prints the Homebrew formula for one release, from the .sha256 files the
# release workflow attaches next to each archive:
#
#   scripts/homebrew-formula.sh v0.8.0 <dir holding the .tar.gz.sha256 files>
#
# The release workflow writes the output to Formula/dataseek.rb in
# lmarkmann/homebrew-tap (docs/reference/release.md).
set -eu

tag=$1
sums=$2
releases="https://github.com/lmarkmann/dataseek/releases/download/$tag"

sha() {
	cut -d' ' -f1 "$sums/dataseek-$1.tar.gz.sha256"
}

cat <<EOF
class Dataseek < Formula
  desc "Search for datasets from the terminal"
  homepage "https://github.com/lmarkmann/dataseek"
  version "${tag#v}"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "$releases/dataseek-aarch64-apple-darwin.tar.gz"
      sha256 "$(sha aarch64-apple-darwin)"
    end
    on_intel do
      url "$releases/dataseek-x86_64-apple-darwin.tar.gz"
      sha256 "$(sha x86_64-apple-darwin)"
    end
  end

  on_linux do
    on_arm do
      url "$releases/dataseek-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "$(sha aarch64-unknown-linux-gnu)"
    end
    on_intel do
      url "$releases/dataseek-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "$(sha x86_64-unknown-linux-gnu)"
    end
  end

  def install
    bin.install "dataseek", "dsk"
    generate_completions_from_executable(bin/"dataseek", "completion")
    generate_completions_from_executable(bin/"dsk", "completion")
    (man1/"dataseek.1").write Utils.safe_popen_read(bin/"dataseek", "man")
    man1.install_symlink "dataseek.1" => "dsk.1"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/dsk --version")
  end
end
EOF
