#!/bin/sh
# Install the synthetic-customers command on macOS or Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/edouard38000/synthetic-customers/main/install.sh | sh
#
# Downloads the prebuilt binary for this computer from the GitHub release,
# checks its SHA-256 checksum, and copies it to ~/.local/bin (no sudo).
#
# Optional settings (environment variables):
#   SYNTHETIC_CUSTOMERS_VERSION      release tag to install, e.g. v0.1.0 (default: latest)
#   SYNTHETIC_CUSTOMERS_INSTALL_DIR  where to put the binary (default: ~/.local/bin)
#
# Needs no token or account; it only downloads public release files.

set -eu

REPO="edouard38000/synthetic-customers"
BIN="synthetic-customers"

say() { printf '%s\n' "$*"; }
fail() {
	printf 'install.sh: error: %s\n' "$*" >&2
	exit 1
}

need() {
	command -v "$1" >/dev/null 2>&1 || fail "this script needs '$1', which was not found"
}

download() {
	# download URL FILE
	if command -v curl >/dev/null 2>&1; then
		curl --proto '=https' --tlsv1.2 -fsSL --retry 3 -o "$2" "$1"
	elif command -v wget >/dev/null 2>&1; then
		wget --https-only -q -O "$2" "$1"
	else
		fail "this script needs curl or wget to download files"
	fi
}

sha256_of() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | cut -d ' ' -f 1
	elif command -v shasum >/dev/null 2>&1; then
		shasum -a 256 "$1" | cut -d ' ' -f 1
	else
		fail "this script needs sha256sum or shasum to check the download"
	fi
}

detect_target() {
	os=$(uname -s)
	arch=$(uname -m)
	case "$os" in
	Darwin)
		# A Terminal running under Rosetta reports x86_64 on Apple Silicon;
		# install the native build anyway.
		if [ "$arch" = "x86_64" ] &&
			[ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = "1" ]; then
			arch="arm64"
		fi
		case "$arch" in
		arm64 | aarch64) echo "aarch64-apple-darwin" ;;
		x86_64) echo "x86_64-apple-darwin" ;;
		*) fail "unsupported Mac CPU: $arch" ;;
		esac
		;;
	Linux)
		case "$arch" in
		x86_64 | amd64) echo "x86_64-unknown-linux-gnu" ;;
		*) fail "no prebuilt binary for Linux on $arch yet; build from source with: cargo install --git https://github.com/$REPO" ;;
		esac
		;;
	MINGW* | MSYS* | CYGWIN*)
		fail "on Windows, use install.ps1 in PowerShell instead (see the README)"
		;;
	*)
		fail "unsupported system: $os"
		;;
	esac
}

main() {
	need uname
	need tar
	need mktemp

	target=$(detect_target)
	version="${SYNTHETIC_CUSTOMERS_VERSION:-latest}"
	install_dir="${SYNTHETIC_CUSTOMERS_INSTALL_DIR:-$HOME/.local/bin}"
	archive="$BIN-$target.tar.gz"

	if [ "$version" = "latest" ]; then
		base="https://github.com/$REPO/releases/latest/download"
	else
		case "$version" in
		v*) ;;
		*) version="v$version" ;;
		esac
		base="https://github.com/$REPO/releases/download/$version"
	fi

	tmp=$(mktemp -d)
	trap 'rm -rf "$tmp"' EXIT INT TERM

	say "Downloading $BIN ($version) for $target..."
	download "$base/$archive" "$tmp/$archive" ||
		fail "could not download $base/$archive (is there a published release $version?)"
	download "$base/SHA256SUMS.txt" "$tmp/SHA256SUMS.txt" ||
		fail "could not download the checksum file $base/SHA256SUMS.txt"

	expected=$(awk -v f="$archive" '$2 == f || $2 == "*" f { print $1 }' "$tmp/SHA256SUMS.txt")
	[ -n "$expected" ] || fail "no checksum listed for $archive"
	actual=$(sha256_of "$tmp/$archive")
	if [ "$expected" != "$actual" ]; then
		fail "checksum mismatch for $archive (expected $expected, got $actual); nothing was installed"
	fi
	say "Checksum OK."

	tar -xzf "$tmp/$archive" -C "$tmp"
	src="$tmp/$BIN-$target/$BIN"
	[ -f "$src" ] || fail "the download did not contain $BIN"

	mkdir -p "$install_dir"
	# Copy then rename, so a running copy is never left half-written.
	cp "$src" "$install_dir/$BIN.tmp"
	chmod 755 "$install_dir/$BIN.tmp"
	mv -f "$install_dir/$BIN.tmp" "$install_dir/$BIN"

	say ""
	say "Installed $BIN to $install_dir/$BIN"
	case ":${PATH:-}:" in
	*":$install_dir:"*)
		say "Run '$BIN --help' to get started."
		;;
	*)
		case "${SHELL:-}" in
		*/zsh) rc="$HOME/.zshrc" ;;
		*/bash) rc="$HOME/.bashrc" ;;
		*) rc="your shell's startup file" ;;
		esac
		say ""
		say "$install_dir is not on your PATH yet. To add it, run:"
		say ""
		# shellcheck disable=SC2016 # $PATH is meant literally here
		say "  echo 'export PATH=\"$install_dir:\$PATH\"' >> $rc"
		say ""
		say "then open a new terminal window. Until then you can run it as:"
		say "  $install_dir/$BIN --help"
		;;
	esac
}

main "$@"
