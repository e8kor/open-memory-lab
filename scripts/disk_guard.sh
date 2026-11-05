#!/usr/bin/env bash
# Refuse to continue when the filesystem holding <path> is fuller than <percent>.
#   scripts/disk_guard.sh <path> [percent]      (default 85)
# Run it before a build in a lane: a full disk fails a build half way and leaves target/ broken.
set -euo pipefail
path="${1:?usage: disk_guard.sh <path> [percent]}"
limit="${2:-85}"
used="$(df -P "$path" | awk 'NR==2 { gsub("%", "", $5); print $5 }')"
if [ "$used" -gt "$limit" ]; then
  echo "disk_guard: $path is ${used}% full (limit ${limit}%); free space before building" >&2
  exit 1
fi
echo "disk_guard: $path is ${used}% full (limit ${limit}%)"
