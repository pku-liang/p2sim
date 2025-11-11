#!/bin/bash

set -xe

out_base=$1

if [ -z "$out_base" ]; then
    echo "Usage: $0 <output-directory>"
    exit 1
fi

mkdir -p $out_base
cp -r proflib $out_base
git rev-parse HEAD > $out_base/git-rev.txt
out_dir=$1/data

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

tar cvf $out_base.tar $out_base
xz -9 $out_base.tar