#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 || $# -gt 3 ]]; then
  echo "usage: $0 SIM OUTPUT_DIR [CYCLES]" >&2
  exit 2
fi

sim=$1
output_dir=$2
cycles=${3:-5000000}
dhry_bin=${DHRY_BIN:-exe/div-1-dhrystone.bin}
coremark_bin=${COREMARK_BIN:-exe/div-1-coremark.bin}
cpuset=${CPUSET:-}

mkdir -p "$output_dir"

run_one() {
  local name=$1
  local image=$2
  local log="$output_dir/$name.out"
  local prof="$output_dir/$name.prof"

  printf 'running %s: sim=%s image=%s cycles=%s\n' \
    "$name" "$sim" "$image" "$cycles"
  local runner=()
  if [[ -n "$cpuset" ]]; then
    runner=(taskset -c "$cpuset")
  fi
  pixi run env \
    MAX_CYCLES="$cycles" \
    PROF_FILE="$prof" \
    "${runner[@]}" "$sim" "$image" 2>&1 | tee "$log"

  local last_cycle
  last_cycle=$(grep '^cycle=' "$log" | tail -1 || true)
  local freq_khz
  freq_khz=$(sed -n 's/.*freq=\([0-9.]*\) kHz.*/\1/p' <<<"$last_cycle")
  printf '%s\t%s\t%s\t%s\n' "$name" "$cycles" "${freq_khz:-NA}" "$image" \
    >>"$output_dir/summary.tsv"

  case "$name" in
    dhry)
      grep -q 'inal values of the variables used in the benchmark' "$log"
      grep -q 'Dhrystones per Second:' "$log"
      ;;
    coremark)
      grep -q '\[0\]crclist       : 0xe714' "$log"
      grep -q '\[0\]crcmatrix     : 0x1fd7' "$log"
      grep -q '\[0\]crcstate      : 0x8e3a' "$log"
      ! grep -q '\]ERROR! .* crc' "$log"
      if grep -q 'Errors detected' "$log"; then
        grep -q 'ERROR! Must execute for at least 10 secs for a valid result!' "$log"
      fi
      ;;
  esac
}

printf 'benchmark\tcycles\tfrequency_khz\timage\n' >"$output_dir/summary.tsv"
run_one dhry "$dhry_bin"
run_one coremark "$coremark_bin"

cat "$output_dir/summary.tsv"
