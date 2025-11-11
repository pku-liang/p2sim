set shell := ["bash", "-uc"]
set unstable

build-gsim:
    pixi run make -C gsim build-gsim

run-all: \
    (run-target-all "rocket_m_1") \
    (run-target-all "rocket_m_2") \
    (run-target-all "rocket_m_4") \
    (run-target-all "boom_m_1") \
    (run-target-all "boom_m_2") \
    (run-target-all "boom_m_4") \
    (run-target-all "boom_l_1") \
    (run-target-all "boom_l_2") \
    (run-target-all "boom_l_4")

run-target-all target: \
    (compile-1234 target "ref") \
    (prof-1234 target "linux.bin" "ref") \
    (compile-1234 target "pgo") \
    (prof-1234 target "linux.bin" "pgo")

prof-1234 target exe sfx="ref": \
    (prof target exe "1" sfx) \
    (prof target exe "2" sfx) \
    (prof target exe "3" sfx) \
    (prof target exe "4" sfx) \
    (prof target exe "5" sfx) \
    (prof target exe "6" sfx) \
    (prof target exe "7" sfx) \
    (prof target exe "8" sfx) \
    (prof target exe "10" sfx) \
    (prof target exe "12" sfx) \
    (prof target exe "14" sfx) \
    (prof target exe "16" sfx) \
    (prof target exe "20" sfx) \
    (prof target exe "24" sfx) \
    (prof target exe "28" sfx)

[parallel]
compile-1234 target sfx="ref": \
    (compile target "1" sfx) \
    (compile target "2" sfx) \
    (compile target "3" sfx) \
    (compile target "4" sfx) \
    (compile target "5" sfx) \
    (compile target "6" sfx) \
    (compile target "7" sfx) \
    (compile target "8" sfx) \
    (compile target "10" sfx) \
    (compile target "12" sfx) \
    (compile target "14" sfx) \
    (compile target "16" sfx) \
    (compile target "20" sfx) \
    (compile target "24" sfx) \
    (compile target "28" sfx)

[script("bash")]
prof target exe threads sfx="ref":
    set -ex
    target_dir=build/{{ target }}.T{{ threads }}
    prof_dir=$target_dir/prof.{{ sfx }}
    target_sim=$target_dir/sim.{{ sfx }}
    bind_cpus=48-$((48 + {{ threads }}))
    MAX_CYCLES=100000 taskset -c $bind_cpus $target_sim ./exe/{{ exe }} || true
    MAX_CYCLES=100000 taskset -c $bind_cpus $target_sim ./exe/{{ exe }} || true
    mkdir -p $prof_dir
    env \
        MAX_CYCLES=5000000 \
        PROF_FILE=$prof_dir/{{ exe }} \
            taskset -c $bind_cpus \
                $target_sim ./exe/{{ exe }} \
                1> >(tee $prof_dir/{{ exe }}.out) \
                2> >(tee $prof_dir/{{ exe }}.log >&2) \
        || true

run target threads="1": (compile target threads)
    ./build/{{ target }}.T{{ threads }}/sim exe/linux.bin

compile target threads="1" sfx="ref":
    rm -rf build/{{ target }}.T{{ threads }}/src
    mkdir -p build/{{ target }}.T{{ threads }}/src
    cargo run --bin ggsim --release -- \
        designs/{{ target }}.ggsim.sexp \
        emu/emu.cc \
        -lfesvr \
        -o build/{{ target }}.T{{ threads }} \
        -k {{ threads }} 2>&1 \
        --prof-act-cnt --prof-rdtsc-avg \
        {{ if sfx == "pgo" { "--pgo" } else { "" } }} \
        --prof-data build/{{ target }}.T{{ threads }}/prof.ref/linux.bin \
        | tee build/{{ target }}.T{{ threads }}/build.{{ sfx }}.log
    pixi run make -C build/{{ target }}.T{{ threads }} || true
    cp build/{{ target }}.T{{ threads }}/sim build/{{ target }}.T{{ threads }}/sim.{{ sfx }}
    echo "Logfile: build/{{ target }}.T{{ threads }}/build.{{ sfx }}.log"

gen target: (build-gsim)
    gsim/build/gsim/gsim --supernode-max-size=1 --output designs/{{ target }}.ggsim.sexp emu/rsrc/{{ target }}.gsim.fir
