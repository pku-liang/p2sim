set shell := ["bash", "-uc"]

build-gsim:
    pixi run make -C gsim build-gsim

prof-1234 target exe: \
    (prof target exe "1") \
    (prof target exe "2") \
    (prof target exe "3") \
    (prof target exe "4") \
    (prof target exe "5") \
    (prof target exe "6") \
    (prof target exe "7") \
    (prof target exe "8")

prof target exe threads:
    MAX_CYCLES=100000 taskset -c 48-68 ./build/{{ target }}.T{{ threads }}/sim ./exe/{{ exe }} || true
    MAX_CYCLES=100000 taskset -c 48-68 ./build/{{ target }}.T{{ threads }}/sim ./exe/{{ exe }} || true
    MAX_CYCLES=100000 taskset -c 48-68 ./build/{{ target }}.T{{ threads }}/sim ./exe/{{ exe }} || true
    mkdir -p $PWD/build/{{ target }}.T{{ threads }}/prof
    env \
        MAX_CYCLES=400000 \
        PROF_FILE=$PWD/build/{{ target }}.T{{ threads }}/prof/{{ exe }} \
            taskset -c 48-68 \
                ./build/{{ target }}.T{{ threads }}/sim ./exe/{{ exe }} \
                1> >(tee $PWD/build/{{ target }}.T{{ threads }}/prof/{{ exe }}.out) \
                2> >(tee $PWD/build/{{ target }}.T{{ threads }}/prof/{{ exe }}.log >&2) \
        || true

[parallel]
compile-1234 target: \
    (compile target "1") \
    (compile target "2") \
    (compile target "3") \
    (compile target "4") \
    (compile target "5") \
    (compile target "6") \
    (compile target "7") \
    (compile target "8")

run target threads="1": (compile target threads)
    ./build/{{ target }}.T{{ threads }}/sim exe/linux.bin

compile target threads="1" pgo="false":
    rm -rf build/{{ target }}.T{{ threads }}/src
    mkdir -p build/{{ target }}.T{{ threads }}/src
    echo "Logfile: build/{{ target }}.T{{ threads }}/build.log"
    cargo run --bin ggsim --release -- \
        designs/{{ target }}.ggsim.sexp \
        emu/emu.cc \
        -lfesvr \
        -o build/{{ target }}.T{{ threads }} \
        -k {{ threads }} 2>&1 \
        --prof-act-cnt \
        --prof-rdtsc-ticks \
        | tee build/{{ target }}.T{{ threads }}/build.log
    pixi run make -C build/{{ target }}.T{{ threads }}
    echo "Logfile: build/{{ target }}.T{{ threads }}/build.log"

# [parallel]
# gen-all: (gen "boom_m_1") (gen "boom_m_2") (gen "boom_m_4") (gen "rocket_m_1") (gen "rocket_m_2") (gen "rocket_m_4") (gen "boom_l_1") (gen "boom_l_2") (gen "boom_l_4")

gen target: (build-gsim)
    gsim/build/gsim/gsim --supernode-max-size=1 --output designs/{{ target }}.ggsim.sexp emu/rsrc/{{ target }}.gsim.fir
