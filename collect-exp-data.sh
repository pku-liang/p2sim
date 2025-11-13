#!/bin/bash

out_base=$1

out_dir=$out_base/data
mkdir -p $out_dir

if [ -z "$out_dir" ]; then
    echo "Usage: $0 <output-directory>"
    exit 1
fi

for ggexp in build/*/prof.*; do
    base=${ggexp##build/}
    base=${base%%/prof.*}
    mkdir -p "$out_dir/$base"
    cp -r "$ggexp" "$out_dir/$base"
done

for vexp in baseline/vlt/build/*/prof.*; do
    base=${vexp##baseline/vlt/build/}
    base=${base%%/prof.*}
    mkdir -p "$out_dir/$base"
    cp -r "$vexp" "$out_dir/$base"
done

for gexp in baseline/gsim/build/*; do
    base=${gexp##baseline/gsim/build/}
    mkdir -p "$out_dir/$base.T1"
    cp -r "$gexp/prof.gsim" "$out_dir/$base.T1/"
done

for rexp in ../repcut-eval/build/*; do
    base=${rexp##../repcut-eval/build/}
    mkdir -p "$out_dir/$base"
    cp -r "$rexp/prof.repcut" "$out_dir/$base"
    cp $out_dir/$base/prof.repcut/{dhrystone,linux.bin}.out
done

cp -r proflib $out_base