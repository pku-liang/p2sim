#!/bin/bash

out_dir=$1

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