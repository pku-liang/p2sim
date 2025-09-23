extract-rsrc:
    cd rsrc && for f in *.tar.zst; do tar xvzf $f; done

compile DESIGN:
    cargo run --release --bin ggsim -- rsrc/{{ DESIGN }}.sexp rsrc/emu.cc -D__DUT_{{ replace(DESIGN, '-', '_') }}__ -o build/{{ DESIGN }}
    make -j $(nproc) -C build/{{ DESIGN }}

run DESIGN: (compile DESIGN)
    build/{{ DESIGN }}/sim rsrc/linux.bin
