build-gsim:
    pixi run make -C gsim build-gsim

run-all profile: (run profile "rocket-m") (run profile "rocket-x") (run profile "boom-m") (run profile "boom-l")

run profile target: (compile profile target)
    ./build/{{ profile }}/{{ target }}/sim exe/coremark.riscv
    ./build/{{ profile }}/{{ target }}/sim exe/dhrystone.riscv

[parallel]
compile-all profile: (compile profile "rocket-m") (compile profile "rocket-x") (compile profile "boom-m") (compile profile "boom-l")

compile profile target:
    pixi run cargo run --release designs/{{ target }}.ggsim.sexp emu/emu.cc -lfesvr -o build/{{ profile }}/{{ target }} --profile {{ profile }} --build

[parallel]
gen-all: (gen "rocket-m") (gen "rocket-x") (gen "boom-m") (gen "boom-l")

gen target: (build-gsim)
    gsim/build/gsim/gsim --output designs/{{ target }}.ggsim.sexp emu/rsrc/{{ target }}.gsim.fir
