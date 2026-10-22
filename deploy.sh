#!/usr/bin/env bash
#
# SPDX-FileCopyrightText: 2026 Thomas Killer
# SPDX-License-Identifier: MIT
#
# Packages a release of git-multi-repo for the host platform:
#   dist/git-multi-repo-<version>-<target>.tar.gz  (binary, README.md, LICENSE)
#   dist/git-multi-repo-<version>-<target>.tar.gz.sha256
#   dist/git-multi-repo_<version>-1_<arch>.deb (+ .sha256), if cargo-deb is installed
#
# The working tree must be clean so every archive corresponds to a commit,
# and formatting, lints and tests must pass before anything is built.

set -euo pipefail

name="git-multi-repo"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$root"

fail() {
    echo "error: $*" >&2
    exit 1
}

for tool in cargo rustc git tar sha256sum; do
    command -v "$tool" >/dev/null || fail "'$tool' is required but not installed"
done

if [[ -n "$(git status --porcelain)" ]]; then
    fail "working tree has uncommitted changes; commit or stash them first"
fi

echo "==> checking formatting, lints and tests"
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked

echo "==> building release binary"
cargo build --release --locked

# `cargo pkgid` prints e.g. `path+file:///…#0.1.0` or `…#name@0.1.0`.
pkgid="$(cargo pkgid)"
version="${pkgid##*#}"
version="${version##*@}"
target="$(rustc -vV | sed -n 's/^host: //p')"
[[ -n "$version" && -n "$target" ]] || fail "cannot determine version or host target"

package="$name-$version-$target"
archive="$package.tar.gz"
dist="$root/dist"
staging="$dist/$package"

echo "==> packaging $archive"
mkdir -p "$dist"
if [[ -e "$dist/$archive" ]]; then
    echo "note: replacing existing dist/$archive"
fi
rm -rf "$staging" "$dist/$archive" "$dist/$archive.sha256"
mkdir "$staging"
cp "target/release/$name" README.md LICENSE "$staging/"
tar -czf "$dist/$archive" -C "$dist" "$package"
rm -rf "$staging"
(cd "$dist" && sha256sum "$archive" >"$archive.sha256")

deb=""
if cargo deb --version >/dev/null 2>&1; then
    echo "==> building Debian package"
    deb_path="$(cargo deb --locked --no-build)"
    deb="$(basename "$deb_path")"
    rm -f "$dist/$deb" "$dist/$deb.sha256"
    cp "$deb_path" "$dist/"
    (cd "$dist" && sha256sum "$deb" >"$deb.sha256")
else
    echo "note: cargo-deb not installed, skipping Debian package (cargo install cargo-deb)"
fi

echo "==> done"
echo "archive:  dist/$archive"
echo "checksum: $(cut -d' ' -f1 "$dist/$archive.sha256")"
if [[ -n "$deb" ]]; then
    echo "debian:   dist/$deb"
    echo "checksum: $(cut -d' ' -f1 "$dist/$deb.sha256")"
fi
