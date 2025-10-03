alias c := compile
alias r := run

compile DESIGN:
    cargo run --release --bin ggsim -- \
        -D__DUT_{{ replace(DESIGN, '-', '_') }}__ \
        rsrc/{{ DESIGN }}.sexp \
        rsrc/emu.cc \
        -o build/{{ DESIGN }}
    pixi run make -j $(nproc) -C build/{{ DESIGN }}

run DESIGN: (compile DESIGN)
    build/{{ DESIGN }}/sim rsrc/linux.bin

init:
    git submodule update --init --recursive
    cd gsim/ready-to-run && pixi run bash ./init.sh
    cd gsim && pixi run bear -- make build-gsim

gen target:
    pixi run make -C gsim compile dutName={{ target }} && cp gsim/build/{{ target }}/model/{{ \
        if target == "ysyx3" { \
            "newtop" \
        } else if target == "rocket" { \
            "TestHarness" \
        } else if target == "NutShell" { \
            "SimTop" \
        } else if target == "small-boom" { \
            "TestHarness" \
        } else if target == "large-boom" { \
            "TestHarness" \
        } else if target == "minimal-xiangshan" { \
            "SimTop" \
        } else if target == "default-xiangshan" { \
            "SimTop" \
        } else { "" }  \
    }}.sch.sexp ./rsrc/{{ target }}.sexp

test target: (gen target) (run target)