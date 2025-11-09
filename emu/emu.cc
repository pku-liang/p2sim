#include <cstring>
#include <cassert>
#include <csignal>
#include <sys/mman.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>
#include <cassert>
#include <iostream>
#include <memory>

// for better syntax highlighting
#ifndef __LINT_ONLY__
#include SIM_INC
#else
struct SIM_NAME {
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

  return {program, program_sz};
}

int main(int argc, char** argv) {
  auto dut = std::make_unique<SIM_NAME>();
  dut->init();
  std::cout << "load program to memory\n";
  Program program = load_program(argv[1]);
  std::cout << "  program_size=" << program.program_sz << "\n";
  void * memory = dut->mem$srams$mem;
  memcpy(memory, program.program, program.program_sz);
  std::cout << "set reset to 1\n";
  dut->set_reset(1);
  dut->step(10);
  std::cout << "set reset to 0\n";
  dut->set_reset(0);
  dut->step(1);
  int max_cycles = 1e6;
  if(getenv("MAX_CYCLES")) {
    max_cycles = std::atoi(getenv("MAX_CYCLES"));
  }
  std::cout << "start simulation: max_cycles=" << max_cycles << "\n";
  dut->step_mt(max_cycles);
  if(getenv("PROF_FILE")) {
    std::cout << "Saving prof data to " << getenv("PROF_FILE") << "\n";
    dut->dump_prof(getenv("PROF_FILE"));
  }
  return 0;
}
