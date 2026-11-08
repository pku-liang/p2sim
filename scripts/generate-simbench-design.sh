#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 DESIGN_NAME CORE_CONFIG" >&2
  exit 2
fi

name=$1
core=$2
simbench_dir=${SIMBENCH_DIR:-../simbench}
ncores=${NCORES:-1}
force=${FORCE:-0}

cd "$simbench_dir"
jar=$(find .pixi/build-v0 -type f \
  -name 'chisel-cores-assembly-*.jar' -print -quit)
firport=$(find .pixi/build-v0 -type f \
  -path '*/work/target/*/release/firport' -print -quit)
test -n "$jar" -a -f "$jar"
test -n "$firport" -a -x "$firport"

target_dir="build/chisel-cores/$name"
if [[ "$force" == 1 ]]; then
  rm -rf "$target_dir"
fi
mkdir -p "$target_dir" designs/gsim

if [[ ! -s "$target_dir/TestHarness.fir" ]]; then
  java -jar "$jar" "$core" -n "$ncores" \
    --with-uart \
    --with-bootrom exe/bootrom/bootrom.img \
    -o "$target_dir"
fi

"$firport" -v -I emu/initial-pats \
  "$target_dir/TestHarness.fir" \
  -o "$target_dir/TestHarness.330.fir" \
  2>"$target_dir/firport.330.log"

"$firport" -v -I emu/gsim/pats \
  "$target_dir/TestHarness.330.fir" \
  -o "designs/gsim/$name.gsim.fir" \
  2>"$target_dir/firport.gsim.log"

test -s "designs/gsim/$name.gsim.fir"
ls -lh "$target_dir/TestHarness.fir" "designs/gsim/$name.gsim.fir"
