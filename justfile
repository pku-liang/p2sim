compile target:
    pixi run cargo run --release designs/{{ target }}.ggsim.sexp -o build/{{ target }}

gen-all:
    just gen rocket-m
    just gen rocket-l
    just gen boom-m
    just gen boom-l

gen target:
    gsim/build/gsim/gsim --output designs/{{ target }}.ggsim.sexp emu/rsrc/{{ target }}.gsim.fir
