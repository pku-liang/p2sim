set shell := ["bash", "-uc"]
set unstable

build-gsim:
    pixi run make -C gsim build-gsim

run-all: \
    (run-target-all "rocket_m_1") \
    (run-target-all "boom_m_1") \
    (run-target-all "boom_l_1")

run-target-all target: \
    (compile-1234 target "0") \
    (prof-1234 target "linux.bin" "0") \
    (compile-1234 target "1") \
    (prof-1234 target "linux.bin" "1")

prof-1234 target exe pgo="0": \
    (prof target exe "1" pgo) \
    (prof target exe "2" pgo) \
    (prof target exe "3" pgo) \
    (prof target exe "4" pgo) \
    (prof target exe "5" pgo) \
    (prof target exe "6" pgo) \
    (prof target exe "7" pgo) \
    (prof target exe "8" pgo) \
    (prof target exe "9" pgo) \
    (prof target exe "10" pgo) \
    (prof target exe "11" pgo) \
    (prof target exe "12" pgo)

[script("bash")]
prof target exe threads pgo="0":
    set -ex
    target_dir=build/{{ target }}.T{{ threads }}
    prof_dir=$target_dir/prof{{ if pgo == "1" { ".pgo" } else { "" } }}
    target_sim=$target_dir/sim{{ if pgo == "1" { ".pgo" } else { ".ref" } }}
    MAX_CYCLES=100000 taskset -c 48-68 $target_sim ./exe/{{ exe }} || true
    MAX_CYCLES=100000 taskset -c 48-68 $target_sim ./exe/{{ exe }} || true
    mkdir -p $prof_dir
    env \
        MAX_CYCLES=10000000 \
        PROF_FILE=$prof_dir/{{ exe }} \
            taskset -c 48-68 \
                $target_sim ./exe/{{ exe }} \
                > >(tee $prof_dir/{{ exe }}.out) \
                2> >(tee $prof_dir/{{ exe }}.log >&2) \
        || true

[parallel]
compile-1234 target pgo="0": \
    (compile target "1" pgo) \
    (compile target "2" pgo) \
    (compile target "3" pgo) \
    (compile target "4" pgo) \
    (compile target "5" pgo) \
    (compile target "6" pgo) \
    (compile target "7" pgo) \
    (compile target "8" pgo) \
    (compile target "9" pgo) \
    (compile target "10" pgo) \
    (compile target "11" pgo) \
    (compile target "12" pgo)

run target threads="1": (compile target threads)
    ./build/{{ target }}.T{{ threads }}/sim exe/linux.bin

compile target threads="1" pgo="0":
    rm -rf build/{{ target }}.T{{ threads }}/src
    mkdir -p build/{{ target }}.T{{ threads }}/src
    echo "Logfile: build/{{ target }}.T{{ threads }}/build.log"
    cargo run --bin ggsim --release -- \
        designs/{{ target }}.ggsim.sexp \
        emu/emu.cc \
        -lfesvr \
        -o build/{{ target }}.T{{ threads }} \
        -k {{ threads }} 2>&1 \
        --prof-act-cnt --prof-rdtsc-avg \
        {{ if pgo == "1" { "--pgo" } else { "" } }} \
        --prof-data build/{{ target }}.T{{ threads }}/prof/linux.bin \
        | tee build/{{ target }}.T{{ threads }}/build.{{ pgo}}.log
    pixi run make -C build/{{ target }}.T{{ threads }}
    echo "Logfile: build/{{ target }}.T{{ threads }}/build.{{ pgo }}.log"

gen target: (build-gsim)
    gsim/build/gsim/gsim --supernode-max-size=1 --output designs/{{ target }}.ggsim.sexp emu/rsrc/{{ target }}.gsim.fir
