#!/bin/bash

for f in build/mt/*/prof.bin; do
    base=${f##build/mt/}
    base=${base%%/prof.bin}
    # echo $base
    if [ -f build/mt/$base/blkid.bin ]; then
        mkdir -p prof/data/$base
        cp build/mt/$base/prof.bin prof/data/$base/prof.bin
        cp build/mt/$base/blkid.bin prof/data/$base/blkid.bin
    fi
done
