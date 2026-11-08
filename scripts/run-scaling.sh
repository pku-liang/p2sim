#!/usr/bin/env bash
set -euo pipefail

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [OUTPUT_ROOT]" >&2
  exit 2
fi

output_root=${1:-build/scaling}
cycles=${CYCLES:-5000000}
threads_text=${THREADS:-"1 2 4 8 16 24"}
cpu_base=${CPU_BASE:-}
force_rebuild=${FORCE_REBUILD:-0}
resume=${RESUME:-0}
overwrite_metadata=${OVERWRITE_METADATA:-0}
make_jobs=${MAKE_JOBS:-24}
generator=${GENERATOR:-target/release/ggsim}
design=${DESIGN:-designs/rocket_m_1.ggsim.sexp}
design_name=${DESIGN_NAME:-$(basename "$design" .ggsim.sexp)}
emu=${EMU:-emu/emu.cc}

read -r -a thread_counts <<<"$threads_text"
if [[ ${#thread_counts[@]} -eq 0 ]]; then
  echo "THREADS must contain at least one thread count" >&2
  exit 2
fi

test -x "$generator"
test -s "$design"
mkdir -p "$output_root/sims" "$output_root/results" "$output_root/logs"

git_rev=${SOURCE_REV:-$(git rev-parse HEAD)}
if [[ "$overwrite_metadata" == 1 || ! -e "$output_root/metadata.tsv" ]]; then
  {
    printf 'git_revision\t%s\n' "$git_rev"
    printf 'design\t%s\n' "$design_name"
    printf 'cycles\t%s\n' "$cycles"
    printf 'threads\t%s\n' "$threads_text"
    printf 'cpu_base\t%s\n' "${cpu_base:-unbound}"
    printf 'hostname\t%s\n' "$(hostname)"
  } >"$output_root/metadata.tsv"
fi

build_one() {
  local threads=$1
  local simulator=$2
  local out="$output_root/sims/${design_name}.T${threads}.${simulator}"
  local log_base="$output_root/logs/T${threads}.${simulator}"

  if [[ "$force_rebuild" != 1 && -x "$out/sim" ]]; then
    printf 'reusing %s\n' "$out/sim"
    return
  fi

  rm -rf "$out"
  mkdir -p "$out/src"
  local args=(
    "$generator"
    "$design"
    "$emu"
    -lfesvr
    -o "$out"
    -k "$threads"
    --prof-rdtsc-avg
  )
  if [[ "$simulator" == spec-sim ]]; then
    args+=(--spec-sim)
  fi

  printf 'generating T%s %s\n' "$threads" "$simulator"
  "${args[@]}" >"$log_base.generate.log" 2>&1
  pixi run env MAKEFLAGS="-j$make_jobs" make -C "$out" \
    >"$log_base.make.log" 2>&1
  test -x "$out/sim"
}

run_one() {
  local threads=$1
  local simulator=$2
  local sim="$output_root/sims/${design_name}.T${threads}.${simulator}/sim"
  local result="$output_root/results/T${threads}/${simulator}"
  local cpuset_value=

  if [[ "$resume" == 1 \
    && -s "$result/summary.tsv" \
    && $(wc -l <"$result/summary.tsv") -eq 3 \
    && -s "$result/dhry.out" \
    && -s "$result/coremark.out" ]] \
    && grep -q 'Dhrystones per Second:' "$result/dhry.out" \
    && grep -q '\[0\]crclist       : 0xe714' "$result/coremark.out" \
    && grep -q '\[0\]crcmatrix     : 0x1fd7' "$result/coremark.out" \
    && grep -q '\[0\]crcstate      : 0x8e3a' "$result/coremark.out"; then
    printf 'reusing completed T%s %s results\n' "$threads" "$simulator"
    return
  fi

  if [[ -n "$cpu_base" ]]; then
    cpuset_value="$cpu_base-$((cpu_base + threads - 1))"
  fi

  rm -rf "$result"
  printf 'benchmarking T%s %s cpuset=%s\n' \
    "$threads" "$simulator" "${cpuset_value:-unbound}"
  CPUSET="$cpuset_value" scripts/run-benchmarks.sh \
    "$sim" "$result" "$cycles"
}

for threads in "${thread_counts[@]}"; do
  if ! [[ "$threads" =~ ^[1-9][0-9]*$ ]]; then
    echo "invalid thread count: $threads" >&2
    exit 2
  fi
  if [[ -n "$cpu_base" && $((cpu_base + threads)) -gt $(nproc) ]]; then
    echo "CPU range exceeds online CPUs for T$threads" >&2
    exit 2
  fi
  build_one "$threads" p2sim
  build_one "$threads" spec-sim
done

for index in "${!thread_counts[@]}"; do
  threads=${thread_counts[$index]}
  if ((index % 2 == 0)); then
    order=(p2sim spec-sim)
  else
    order=(spec-sim p2sim)
  fi
  for simulator in "${order[@]}"; do
    run_one "$threads" "$simulator"
  done
done

scripts/collect-scaling.py \
  "$output_root/results" "$output_root/scaling.csv" \
  --design "$design_name"
