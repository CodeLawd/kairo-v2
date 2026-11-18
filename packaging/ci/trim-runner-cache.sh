#!/usr/bin/env bash
# Keeps a self-hosted runner's build output bounded (the Mac runner for the
# arm64 jobs, which keeps its Cargo target directory between runs).
#
#   trim-runner-cache.sh <dir> <limit-GiB> [<min-free-GiB>]
#
# Deletes <dir> when it is larger than <limit-GiB>, so the next build starts
# clean. When the disk has less than <min-free-GiB> free (default 20), it
# deletes every kept build under the parent of <dir> as well, and fails if
# that still leaves too little room.
set -euo pipefail

dir="$1"
limit="$2"
min_free="${3:-20}"
root="$(dirname "$dir")"

gib_of() { du -sk "$1" 2>/dev/null | awk '{ printf "%d", $1 / 1048576 }'; }
free_gib() { df -k "$1" | awk 'NR == 2 { printf "%d", $4 / 1048576 }'; }

mkdir -p "$dir"
used="$(gib_of "$dir")"
echo "build output in $dir: ${used} GiB (limit ${limit} GiB)"
if [ "$used" -gt "$limit" ]; then
  echo "::notice::removing $dir (${used} GiB > ${limit} GiB); this build starts clean"
  rm -rf "$dir"
  mkdir -p "$dir"
fi

free="$(free_gib "$root")"
echo "free disk: ${free} GiB (minimum ${min_free} GiB)"
if [ "$free" -lt "$min_free" ]; then
  echo "::warning::only ${free} GiB free; removing every kept build under $root"
  rm -rf "${root:?}"/*
  mkdir -p "$dir"
  free="$(free_gib "$root")"
  if [ "$free" -lt "$min_free" ]; then
    echo "::error::only ${free} GiB free on the runner after cleanup; free some disk space"
    exit 1
  fi
fi
