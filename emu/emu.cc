#include <time.h>
#include <cstring>
#include <cassert>
#include <csignal>
#include <sys/mman.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>
#include <cassert>
#include <chrono>
#include <iostream>

// for better syntax highlighting
#ifndef __LINT_ONLY__
#include "TestHarness.h"
#else
struct TestHarness {
  void init();
  void step(int cycles);
  void step_mt(int cycles);
  void set_reset(int reset);
  void dump_prof(const char * filename);
  void * mem$srams$mem;
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

  printf("load program size: 0x%x\n", program_sz);
  return {program, program_sz};
}

static TestHarness * dut = nullptr;

int main(int argc, char** argv) {
  dut = new TestHarness();
  dut->init();
  printf("load program to memory\n");
  Program program = load_program(argv[1]);
  void * memory = dut->mem$srams$mem;
  memcpy(memory, program.program, program.program_sz);
  printf("set reset to 1\n");
  dut->set_reset(1);
  dut->step(10);
  printf("set reset to 0\n");
  dut->set_reset(0);
  dut->step(1);
  int cycle = 0;
  int max_cycles = 1e6;
  if(getenv("MAX_CYCLES")) {
    max_cycles = std::atoi(getenv("MAX_CYCLES"));
  }
  printf("start simulation\n");
  dut->step_mt(max_cycles);
  if(getenv("PROF_FILE")) {
    std::cout << "Saving prof data to " << getenv("PROF_FILE") << std::endl;
    dut->dump_prof(getenv("PROF_FILE"));
  }
  return 0;
}
