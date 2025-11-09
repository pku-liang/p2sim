#include <fcntl.h>
#include <cassert>
#include <iostream>
#include <cstdint>
#include <sys/mman.h>
#include <sys/stat.h>
#include <cstring>
#include <chrono>

#ifndef __LINT_ONLY__
#include "verilated.h"
#include "VTestHarness___024root.h"
#include "VTestHarness.h"
#else 
struct VTestHarness {
  struct {
    char TestHarness__DOT__mem__DOT__srams__DOT__mem_ext__DOT__Memory[];
  } * rootp;
  uint8_t clock;
  uint8_t reset;
  uint8_t io_success;
  void eval();
};
#endif

struct Program {
  void *program;
  int program_sz;
};

static Program load_program(const char *filename) {
  assert(filename != NULL);
  int program_fd = open(filename, O_RDONLY);
  assert(program_fd != -1);

  struct stat s;
  int ret = fstat(program_fd, &s);
  assert(ret == 0);
  int program_sz = s.st_size;

  void *program = mmap(NULL, program_sz, PROT_READ, MAP_PRIVATE, program_fd, 0);
  assert(program != (void *)-1);

  std::cout << "load program size: 0x" << std::hex << program_sz << std::dec << "\n";
  return {program, program_sz};
}

static VTestHarness * dut = nullptr;

struct perf_monitor {
  using clock = std::chrono::high_resolution_clock;
  clock::time_point start;
  clock::time_point last;
  int cycle;
  int cycle_counter;
  int print_interval;
  perf_monitor(int print_interval=100000) : 
    start(clock::now()),
    last(start),
    cycle(0),
    cycle_counter(0),
    print_interval(print_interval) {}
  void reset() {
    start = clock::now();
    last = start;
    cycle = 0;
    cycle_counter = 0;
  }
  void print() {
    auto current = clock::now();
    auto dur_start = std::chrono::duration_cast<std::chrono::microseconds>(current - start).count();
    auto dur_last = std::chrono::duration_cast<std::chrono::microseconds>(current - last).count();
    auto freq = cycle * 1.0 / dur_start * 1e3;
    auto last_freq = print_interval * 1.0 / dur_last * 1e3;
    std::cout << "cycle=" << cycle << ", last_freq=" << last_freq << " kHz, freq=" << freq << " kHz\n";
    last = current;
  }
  void tick() {
    cycle++;
    cycle_counter++;
    if(cycle_counter == print_interval) [[unlikely]] {
      cycle_counter = 0;
      print();
    }
  }
};

void dut_cycle(int cycles) {
  perf_monitor monitor;
  for(int i = 0; i < cycles; i++) {
    dut->clock = 0;
    dut->eval();
    dut->clock = 1;
    dut->eval();
    monitor.tick();
  }
}

int main(int argc, char ** argv) {
#ifndef __LINT_ONLY__
  Verilated::commandArgs(argc, argv);
#endif
  std::cout << "load program to memory\n";
  Program program = load_program(argv[1]);
  std::cout << "  program_size=" << program.program_sz << "\n";
  dut = new VTestHarness{};
  void * memory = (void *)&dut->rootp->TestHarness__DOT__mem__DOT__srams__DOT__mem_ext__DOT__Memory;
  memcpy(memory, program.program, program.program_sz);
  std::cout << "set reset to 1\n";
  dut->reset = 1;
  dut_cycle(10);
  std::cout << "set reset to 0\n";
  dut->reset = 0;
  dut_cycle(1);
  int max_cycles = 1e6;
  if(getenv("MAX_CYCLES")) {
    max_cycles = std::atoi(getenv("MAX_CYCLES"));
  }
  std::cout << "start simulation: max_cycles=" << max_cycles << "\n";
  dut_cycle(max_cycles);
  return 0;
}
