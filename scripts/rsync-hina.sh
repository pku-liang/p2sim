#!/usr/bin/env bash
set -euo pipefail

host=${HINA_HOST:-hina}
dest=${HINA_P2SIM_DIR:-~/pro/p2sim-next/p2sim}

rsync_args=(
  -az
  --checksum
  --human-readable
  --exclude=__pycache__
  --exclude='*.pyc'
)

if [[ ${DRY_RUN:-0} == 1 ]]; then
  rsync_args+=(--dry-run --itemize-changes)
fi

ssh "$host" "test -d $dest/.git"

rsync "${rsync_args[@]}" \
  Cargo.toml \
  Cargo.lock \
  justfile \
  pixi.toml \
  pixi.lock \
  src \
  templates \
  emu \
  scripts \
  spec-sim.md \
  "$host:$dest/"

printf 'synced p2sim sources to %s:%s\n' "$host" "$dest"
