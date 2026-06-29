# ggsim

> **C++ Simulator Generator for RTL Designs — from S-expression to cycle-accurate C++ simulation.**

`ggsim` compiles hardware designs (represented as S-expressions) into high-performance, multi-threaded C++ simulators. It is designed for fast simulation of large RISC-V processors, using automatic logic partitioning to expose parallelism and Askama templates for code generation.

## Supported Designs

| Design | Description |
|--------|-------------|
| `ysyx3` | YSYX SoC (一生一芯) |
| `rocket` | Rocket Chip |
| `NutShell` | NutShell (XiangShan team) |
| `small-boom` / `large-boom` | Berkeley BOOM |
| `minimal-xiangshan` / `default-xiangshan` | XiangShan (香山) |

## Architecture

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│   .sexp      │ --> │    parse     │ --> │   repcut     │ --> │    cgen      │ -->  C++ simulator
│ (design IR)  │     │ (serde_lexpr)│     │ (partition)  │     │ (Askama tmpl)│     (.h/.cc/Makefile)
└──────────────┘     └──────────────┘     └──────────────┘     └──────────────┘
```

### Pipeline

1. **Parse** (`compile.rs`) — Reads S-expression files (`.sexp`/`.sexpr`) into the IR using `serde_lexpr`.
2. **RepCut** (`repcut.rs`) — Partitioning algorithm that groups logic nodes into clusters based on dependency graph analysis, maximizing parallelism while respecting cost thresholds.
3. **CodeGen** (`cgen.rs`) — Generates C++ simulation code using [Askama](https://github.com/rinja-rs/askama) templates, producing header files, implementation files, and a Makefile.
4. **Analysis** (`analysis.rs`) — Computes graph metrics (degree, depth, topological generation) and can dump dependency graphs in Gephi CSV format for visualization.

### IR Data Model (`ir.rs`)

- **`Design`** — Top-level design: name, states, logic nodes, reset logic
- **`State`** — Hardware state element (register, memory, input/output) with width and read/write/activation logic
- **`Logic`** — Combinational logic block with instructions (`Inst::Raw`, `Inst::Save`, `Inst::Act`)
- **`ResetLogic`** — Reset-specific logic for initialization

## Quick Start

### Prerequisites

- [pixi](https://pixi.sh/) (conda-based environment manager)
- [just](https://github.com/casey/just) (command runner)
- Rust (via rustup)

### Setup

```bash
# Install dependencies and initialize submodules
just init

# Generate .sexp for a target design (e.g., ysyx3)
just gen ysyx3
```

### Build & Run

```bash
# Compile and generate C++ simulator for a design
just c ysyx3

# Run the simulator with the test program
just r ysyx3
```

### Manual CLI Usage

```bash
cargo run --release --bin ggsim -- \
    -D__DUT_ysyx3__ \
    rsrc/ysyx3.sexp \
    rsrc/emu.cc \
    -o build/ysyx3
```

**Options:**
- `-D <macro>` — Define a C preprocessor macro
- `-o <dir>` — Output directory for generated C++ files
- `--dump-gephi <prefix>` — Dump dependency graph as Gephi node/edge CSV files

## Project Structure

```
├── src/
│   ├── main.rs          # CLI entry point
│   ├── lib.rs           # Library root, utility traits & macros
│   ├── compile.rs       # S-expression parser
│   ├── ir.rs            # Intermediate representation types
│   ├── repcut.rs        # Logic partitioning (Replication Cut)
│   ├── cgen.rs          # C++ code generation
│   └── analysis.rs      # Graph analysis & metrics
├── rsrc/
│   ├── emu.cc           # C++ emulator harness (main loop, memory, I/O)
│   └── linux.bin        # Test program (Linux kernel image)
├── templates/           # Askama Jinja2 templates for C++ codegen
│   ├── cgen-logic.cpp.j2
│   ├── cgen-struct-decl.h.j2
│   ├── cgen-struct-impl.h.j2
│   ├── cgen-struct-impl.cpp.j2
│   ├── cgen-makefile.mk.j2
│   └── gprintf.j2
├── deprecated/          # Old partitioning experiments
├── pixi.toml            # pixi environment config (clang, lld, ninja, etc.)
├── justfile             # Task definitions (just)
├── Cargo.toml           # Rust dependencies
└── gsim/                # Submodule: gsim — RTL-to-sexp front-end
```

## Key Dependencies

| Dependency | Purpose |
|------------|---------|
| `serde_lexpr` | S-expression deserialization |
| `askama` | Jinja2 template engine for C++ codegen |
| `petgraph` | Graph data structures for dependency analysis |
| `priority-queue` | Priority queue for the RepCut partitioning algorithm |
| `rayon` | Parallel iterators for concurrent code generation & analysis |
| `clap` | CLI argument parsing |
| `memmap2` | Memory-mapped file I/O for fast parsing |

## How RepCut Works

RepCut (Replication Cut) is the core partitioning algorithm:

1. **Build dependency graph** — Nodes are logic blocks; edges are read-after-write data dependencies.
2. **Transitive closure** — Computes reachable sink nodes for every logic node.
3. **Merge pinned nodes** — Always-active or large-array logic is merged eagerly.
4. **Iterative clustering** — Remaining nodes are merged by IoU (Intersection-over-Union) of their sink sets, subject to a union-size threshold, until the partition stabilizes.

This produces clusters of logic that can be evaluated independently, enabling multi-threaded simulation.

## License

See source files for license information.
