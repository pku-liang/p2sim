build-gsim:
    pixi run make -C gsim build-gsim

run-all profile: (run profile "rocket-m") (run profile "rocket-x") (run profile "boom-m") (run profile "boom-l")

run profile target: (compile profile target)
    ACT_INFO_FILE=build/{{ profile }}/{{ target }}/coremark.bin  MAX_CYCLES=2000000 pixi run ./build/{{ profile }}/{{ target }}/sim exe/coremark.riscv
    ACT_INFO_FILE=build/{{ profile }}/{{ target }}/dhrystone.bin MAX_CYCLES=2000000 pixi run ./build/{{ profile }}/{{ target }}/sim exe/dhrystone.riscv

[parallel]
compile-all profile: (compile profile "rocket-m") (compile profile "rocket-x") (compile profile "boom-m") (compile profile "boom-l")

compile profile target:
    rm -rf build/{{ profile }}/{{ target }}
    cargo run --release -- \
        designs/{{ target }}.ggsim.sexp \
        emu/emu.cc \
        -lfesvr \
        -o build/{{ profile }}/{{ target }} \
        --profile {{ profile }} \
        -k 2
    pixi run make -C build/{{ profile }}/{{ target }}

[parallel]
gen-all: (gen "rocket-m") (gen "rocket-x") (gen "boom-m") (gen "boom-l")

gen target: (build-gsim)
    gsim/build/gsim/gsim --supernode-max-size=1 --output designs/{{ target }}.ggsim.sexp emu/rsrc/{{ target }}.gsim.fir

[parallel]
test-all profile: (test profile "rocket-m") (test profile "rocket-x") (test profile "boom-m") (test profile "boom-l")

test profile target: (gen target) (run profile target) 