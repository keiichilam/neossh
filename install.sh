#!/bin/sh
# Install NEO-SSH (consolidated from ssh4 / ssh4gui / ssh5).
set -eu
umask 077
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
install_dir=${HOME:?HOME must be set}/.local/bin
source_file=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --install-dir|--binary)
            [ "$#" -ge 2 ] && [ -n "$2" ] || { echo "Missing value for $1" >&2; exit 1; }
            if [ "$1" = --install-dir ]; then install_dir=$2; else source_file=$2; fi
            shift 2 ;;
        --help)
            echo 'Usage: sh install.sh [--install-dir DIRECTORY] [--binary PREBUILT_EXECUTABLE]'
            exit 0 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done
expected=$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' "$project_dir/Cargo.toml" | head -n 1)
[ -n "$expected" ] || { echo 'Cannot read application version.' >&2; exit 1; }
if [ -z "$source_file" ]; then
    cargo build --release --locked --manifest-path "$project_dir/Cargo.toml" --target-dir "$project_dir/target"
    source_file=$project_dir/target/release/neo-ssh
fi
case "$source_file" in /*) ;; *) source_file=$PWD/$source_file ;; esac
case "$install_dir" in /*) ;; *) install_dir=$PWD/$install_dir ;; esac
[ -f "$source_file" ] || { echo 'Source executable is missing.' >&2; exit 1; }
mkdir -p "$install_dir"
install_dir=$(CDPATH= cd -- "$install_dir" && pwd -P)
destination=$install_dir/neo-ssh
[ ! -d "$destination" ] && [ ! -L "$destination" ] || { echo 'Destination must not be a directory or symbolic link.' >&2; exit 1; }
stage_dir=$(mktemp -d "$install_dir/.ssh4-install-XXXXXXXX")
cleanup() { rm -f "$stage_dir/neo-ssh"; rmdir "$stage_dir"; }
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
cp "$source_file" "$stage_dir/neo-ssh"
chmod 755 "$stage_dir/neo-ssh"
cmp -s "$source_file" "$stage_dir/neo-ssh" || { echo 'Staged executable differs from source.' >&2; exit 1; }
version=$("$stage_dir/neo-ssh" --version)
[ "$version" = "neo-ssh $expected" ] || { echo 'Executable version does not match Cargo.toml; rebuild first.' >&2; exit 1; }
mv -f "$stage_dir/neo-ssh" "$destination"
printf 'Installed %s at %s\n' "$version" "$destination"
printf 'Add this directory to PATH if needed: %s\n' "$install_dir"
