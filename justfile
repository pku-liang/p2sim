build-gsim:
    pixi run make -C gsim build-gsim

prof-1234 target: (prof target "1") (prof target "2") (prof target "3") (prof target "4") (prof target "5") (prof target "6") (prof target "7") (prof target "8") (prof target "9") (prof target "10") (prof target "11") (prof target "12")
    mkdir -p prof
    python scripts/plot-prof.py build/mt/{{ target }}.* prof/{{ target }}

prof target threads:
    MAX_CYCLES=100000 taskset -c 48-68 ./build/mt/{{ target }}.T{{ threads }}/sim ./exe/div-3/dhrystone.bin
    MAX_CYCLES=100000 taskset -c 48-68 ./build/mt/{{ target }}.T{{ threads }}/sim ./exe/div-3/dhrystone.bin
    MAX_CYCLES=100000 taskset -c 48-68 ./build/mt/{{ target }}.T{{ threads }}/sim ./exe/div-3/dhrystone.bin
    MAX_CYCLES=400000 taskset -c 48-68 ./build/mt/{{ target }}.T{{ threads }}/sim ./exe/div-3/dhrystone.bin 2>&1 | tee build/mt/{{ target }}.T{{ threads }}/prof.log

[parallel]
compile-1234 target: (compile target "1") (compile target "2") (compile target "3") (compile target "4") (compile target "5") (compile target "6") (compile target "7") (compile target "8") (compile target "9") (compile target "10") (compile target "11") (compile target "12")

compile target threads="1" profile="mt":
    rm -rf build/{{ profile }}/{{ target }}.T{{ threads }}
    mkdir -p build/{{ profile }}/{{ target }}.T{{ threads }}
    echo "Logfile: build/{{ profile }}/{{ target }}.T{{ threads }}/build.log"
    cargo run --release -- \
        designs/{{ target }}.ggsim.sexp \
        emu/emu.cc \
        -lfesvr \
        -o build/{{ profile }}/{{ target }}.T{{ threads }} \
        --profile {{ profile }} \
        -k {{ threads }} 2>&1 \
        --prof \
        | tee build/{{ profile }}/{{ target }}.T{{ threads }}/build.log
    pixi run make -C build/{{ profile }}/{{ target }}.T{{ threads }}
    echo "Logfile: build/{{ profile }}/{{ target }}.T{{ threads }}/build.log"

[parallel]
gen-all: (gen "boom_m_1") (gen "boom_m_2") (gen "boom_m_4") (gen "rocket_m_1") (gen "rocket_m_2") (gen "rocket_m_4") (gen "boom_l_1") (gen "boom_l_2") (gen "boom_l_4")

gen target: (build-gsim)
    gsim/build/gsim/gsim --supernode-max-size=1 --output designs/{{ target }}.ggsim.sexp emu/rsrc/{{ target }}.gsim.fir
