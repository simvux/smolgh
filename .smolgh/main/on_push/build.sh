#!/bin/env zsh

set -x

cargo build --release --target x86_64-unknown-linux-gnu || exit 1

cp target/x86_64-unknown-linux-gnu/release/smolgh target/x86_64-unknown-linux-gnu/release/smolgh-x86_64-linux-glibc || exit 1

gh release delete-asset main smolgh-x86_64-linux-glibc -y
gh release upload main target/x86_64-unknown-linux-gnu/release/smolgh-x86_64-linux-glibc --clobber
# gh release create main target/release/smolgh-x86_64-linux-glibc --title "main" --notes "Binaries for latest commit on main branch"
