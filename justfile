build-gsim:
    pixi run make -C gsim build-gsim

compile-all: (compile "rocket-m") (compile "rocket-x") (compile "boom-m") (compile "boom-l")

compile target:
    pixi run cargo run --release designs/{{ target }}.ggsim.sexp emu/emu.cc -lfesvr -o build/{{ target }} --build

[parallel]
gen-all: (gen "rocket-m") (gen "rocket-x") (gen "boom-m") (gen "boom-l")

gen target: (build-gsim)
    gsim/build/gsim/gsim --output designs/{{ target }}.ggsim.sexp emu/rsrc/{{ target }}.gsim.fir
