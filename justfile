build-gsim:
    pixi run make -C gsim build-gsim

prof-1234 target: (prof target "1") (prof target "2") (prof target "3") (prof target "4") (prof target "5") (prof target "6")
    mkdir -p prof
    python scripts/plot-prof.py build/{{ target }}.* prof/{{ target }}

prof target threads:
    MAX_CYCLES=100000 taskset -c 48-68 ./build/{{ target }}.T{{ threads }}/sim ./exe/linux.bin || true
    MAX_CYCLES=100000 taskset -c 48-68 ./build/{{ target }}.T{{ threads }}/sim ./exe/linux.bin || true
    MAX_CYCLES=100000 taskset -c 48-68 ./build/{{ target }}.T{{ threads }}/sim ./exe/linux.bin || true
    MAX_CYCLES=4000000 taskset -c 48-68 ./build/{{ target }}.T{{ threads }}/sim ./exe/linux.bin 2>&1 \
        | tee build/mt/{{ target }}.T{{ threads }}/prof.log || true

[parallel]
compile-1234 target: (compile target "1") (compile target "2") (compile target "3") (compile target "4") (compile target "5") (compile target "6")

compile target threads="1" pgo="false":
    rm -rf build/{{ target }}.T{{ threads }}
    mkdir -p build/{{ target }}.T{{ threads }}
    echo "Logfile: build/{{ target }}.T{{ threads }}/build.log"
    cargo run --bin ggsim --release -- \
        designs/{{ target }}.ggsim.sexp \
        emu/emu.cc \
        -lfesvr \
        -o build/{{ target }}.T{{ threads }} \
        -k {{ threads }} 2>&1 \
        --prof \
        --pgo \
        --pgo-id-map prof/data/{{ target }}.T1/blkid.bin \
        --pgo-prof-data prof/data/{{ target }}.T1/prof.bin \
        | tee build/{{ target }}.T{{ threads }}/build.log
    pixi run make -C build/{{ target }}.T{{ threads }}
    echo "Logfile: build/{{ target }}.T{{ threads }}/build.log"

# [parallel]
# gen-all: (gen "boom_m_1") (gen "boom_m_2") (gen "boom_m_4") (gen "rocket_m_1") (gen "rocket_m_2") (gen "rocket_m_4") (gen "boom_l_1") (gen "boom_l_2") (gen "boom_l_4")

gen target: (build-gsim)
    gsim/build/gsim/gsim --supernode-max-size=1 --output designs/{{ target }}.ggsim.sexp emu/rsrc/{{ target }}.gsim.fir
