#!/usr/bin/env bash
# Repackage the gpui-pre crates in this tree and compare each .crate file with
# the one published on crates.io, byte for byte.
#
#   script/verify-upstream.sh [-C DIR] [VERSION]
#
# DIR is the tree to check, by default the one this script is in. Point it at
# a worktree of the `upstream` branch to check an import from main's copy of
# the script. VERSION defaults to the version the workspace pins gpui-pre to.
# On an "as released" commit every crate should match. On main, a crate the
# fork has changed shows up as different, which is the list of crates to
# review.
#
# Packaging only resolves dependencies and writes archives, so this compiles
# nothing. It needs git, cargo, curl, jq and sha256sum.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
if [ "${1:-}" = "-C" ]; then
  root=$(cd "$2" && pwd)
  shift 2
fi
cd "$root"
agent="mhr_gpui verify-upstream"

version=${1:-$(sed -n 's/^gpui = {.*package = "gpui-pre", version = "=\([^"]*\)".*/\1/p' Cargo.toml)}
[ -n "$version" ] || { echo "verify-upstream: cannot read the gpui-pre version from Cargo.toml" >&2; exit 2; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

mapfile -t crates < <(cargo metadata --no-deps --format-version 1 \
  | jq -r --arg v "$version" '.packages[] | select(.name | startswith("gpui-pre")) | select(.version == $v) | .name' \
  | sort)
[ ${#crates[@]} -gt 0 ] || { echo "verify-upstream: no gpui-pre crates at $version in this workspace" >&2; exit 2; }

# Package a copy outside git: inside a repository cargo adds a
# .cargo_vcs_info.json that the published crates don't have.
mkdir "$work/tree"
git ls-files -co --exclude-standard -z | tar --null -T - -cf - | tar -C "$work/tree" -xf -
args=()
for crate in "${crates[@]}"; do args+=(-p "$crate"); done
echo "Packaging ${#crates[@]} crates at $version"
(cd "$work/tree" && cargo package --no-verify --allow-dirty --quiet --target-dir "$work/target" "${args[@]}")

# The sparse index path of a crate name, e.g. gpui-pre -> gp/ui/gpui-pre.
index_path() {
  local name=$1
  case ${#name} in
    1) echo "1/$name" ;;
    2) echo "2/$name" ;;
    3) echo "3/${name:0:1}/$name" ;;
    *) echo "${name:0:2}/${name:2:2}/$name" ;;
  esac
}

same=0 differ=0
for crate in "${crates[@]}"; do
  file="$crate-$version.crate"
  published="$work/$file"
  curl -fsSL -A "$agent" -o "$published" "https://static.crates.io/crates/$crate/$file"
  expected=$(curl -fsSL -A "$agent" "https://index.crates.io/$(index_path "$crate")" \
    | jq -r --arg v "$version" 'select(.vers == $v) | .cksum')
  actual=$(sha256sum "$published" | cut -d' ' -f1)
  if [ "$expected" != "$actual" ]; then
    echo "verify-upstream: $file from crates.io does not match the index checksum" >&2
    exit 1
  fi
  if cmp -s "$published" "$work/target/package/$file"; then
    same=$((same + 1))
    continue
  fi
  differ=$((differ + 1))
  # A crate's packaged Cargo.lock records its dependencies' checksums, so a
  # change to one crate also changes the archives of the crates using it.
  mkdir -p "$work/a" "$work/b"
  tar -xzf "$published" -C "$work/a"
  tar -xzf "$work/target/package/$file" -C "$work/b"
  if diff -rq -x Cargo.lock "$work/a/$crate-$version" "$work/b/$crate-$version" >/dev/null; then
    echo "differs from crates.io in Cargo.lock only (a dependency changed): $crate"
  else
    echo "differs from crates.io: $crate"
    diff -rq -x Cargo.lock "$work/a/$crate-$version" "$work/b/$crate-$version" | sed "s|$work/[ab]/||g; s/^/    /" || true
  fi
done
echo "$same of ${#crates[@]} crates are byte-identical to crates.io, $differ differ"
[ "$differ" -eq 0 ]
