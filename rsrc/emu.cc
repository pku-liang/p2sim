#include <iostream>
#include <time.h>
#include <cstring>
#include <cassert>
#include <vector>
#include <numeric>
#include <algorithm>
#include <fstream>
#include <csignal>
#include <chrono>
#include <sys/mman.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>

#define CYCLE_STEP_PERCENT 1

#if defined(__DUT_ysyx3__)
#define DUT_MEMORY get_mem$ram
#define REF_MEMORY newtop__DOT__mem__DOT__ram_ext__DOT__Memory
#define CYCLE_MAX_PERF 5000000
#define CYCLE_MAX_SIM  11000000
#elif defined(__DUT_NutShell__)
#define DUT_MEMORY get_mem$rdata_mem$mem
#define REF_MEMORY SimTop__DOT__mem__DOT__rdata_mem__DOT__mem_ext__DOT__Memory
#define CYCLE_MAX_PERF 50000000
#define CYCLE_MAX_SIM  250000000
#elif defined(__DUT_rocket__)
#define DUT_MEMORY get_mem$srams$mem
#define REF_MEMORY TestHarness__DOT__mem__DOT__srams__DOT__mem_ext__DOT__Memory
#define CYCLE_MAX_PERF 2000000
#define CYCLE_MAX_SIM  4200000
#elif defined(__DUT_large_boom__) || defined(__DUT_small_boom__)
#define DUT_MEMORY get_mem$srams$mem
#define REF_MEMORY TestHarness__DOT__mem__DOT__srams__DOT__mem_ext__DOT__Memory
#define CYCLE_MAX_PERF 1000000
#ifdef __DUT_large_boom__
#define CYCLE_MAX_SIM  3900000
#else
#define CYCLE_MAX_SIM  5400000
#endif
#elif defined(__DUT_minimal_xiangshan__) || defined(__DUT_default_xiangshan__)
#define DUT_MEMORY get_memory$ram$rdata_mem$mem
#define REF_MEMORY SimTop__DOT__memory__DOT__ram__DOT__rdata_mem__DOT__mem_ext__DOT__Memory
#define CYCLE_MAX_PERF 500000
#ifdef __DUT_default_xiangshan__
#define CYCLE_MAX_SIM  1900000
#else
#define CYCLE_MAX_SIM  3400000
#endif

// unused blackbox
void imsic_csr_top(int EID_VLD_DLY_NUM, int NR_HARTS, int NR_INTP_FILES, int NR_SRC, int XLEN, uint8_t _0, uint8_t _1, uint16_t _2, uint8_t _3, uint8_t _4, uint16_t _5, uint8_t _6, uint8_t _7, uint8_t _8, uint8_t _9, uint8_t _10, uint8_t _11, uint64_t _12, uint8_t& _13, uint64_t& _14, uint8_t& _15, uint8_t& _16, uint32_t& _17, uint32_t& _18, uint32_t& _19) {
  _13 = 0;
  _15 = 0;
  _16 = 0; // o_irq
  _17 = 0;
  _18 = 0;
  _19 = 0;
}

// unused blackbox
void SimJTAG(int param, uint8_t _0, uint8_t& _1, uint8_t& _2, uint8_t& _3, uint8_t _4, uint8_t _5, uint8_t _6, uint8_t _7, uint32_t& _8) {
  _1 = 1; // TRSTn
  _2 = 0; // TMS
  _3 = 0; // TDI
  _8 = 0;
}

// unused blackbox
void FlashHelper(uint8_t en, uint32_t addr, uint64_t &data) {
  data = 0;
}

// unused blackbox
void SDCardHelper(uint8_t setAddr, uint32_t addr, uint8_t ren, uint32_t& data) {
  data = 0;
}

void PrintCommitIDModule(uint8_t _0, uint64_t _1, uint8_t _2) { }

extern "C" void flash_read(unsigned int addr, unsigned long long* data){
  printf("WARNING: Trying to invoke flash_read\n");
}

extern "C" void sd_setaddr(int addr){
  printf("WARNING: Trying to invoke sd_setaddr\n");
}

extern "C" void sd_read(int* data){
  printf("WARNING: Trying to invoke sd_read\n");
}

#else
#error Unsupport DUT = DUT_NAME
#endif

#include DUT_HEADER
static DUT_NAME* dut;

void dut_init(DUT_NAME *dut) {
#if defined(__DUT_NutShell__)
  dut->set_difftest$$logCtrl$$begin(0);
  dut->set_difftest$$logCtrl$$end(0);
  dut->set_difftest$$uart$$in$$ch(-1);
#elif defined(__DUT_minimal_xiangshan__) || defined(__DUT_default_xiangshan__)
  // dut->set_difftest$$perfCtrl$$clean(0);
  // dut->set_difftest$$perfCtrl$$dump(0);
  // dut->set_difftest$$logCtrl$$begin(0);
  // dut->set_difftest$$logCtrl$$end(0);
  // dut->set_difftest$$logCtrl$$level(0);
  dut->set_difftest$$uart$$in$$ch(-1);
#endif
}

void dut_hook(DUT_NAME *dut) {
#if defined(__DUT_NutShell__)
  if (dut->get_difftest$$uart$$out$$valid()) {
    printf("%c", dut->get_difftest$$uart$$out$$ch());
    fflush(stdout);
  }
#elif defined(__DUT_minimal_xiangshan__) || defined(__DUT_default_xiangshan__)
  if (dut->get_difftest$$uart$$out$$valid()) {
    printf("%c", dut->get_difftest$$uart$$out$$ch());
    fflush(stdout);
  }
#endif
}

static int program_sz = 0;
static int program_fd = 0;
static void *program = NULL;
static bool dut_end = false;

template <typename T>
std::vector<size_t> sort_indexes(const std::vector<T> &v) {

  // initialize original index locations
  std::vector<size_t> idx(v.size());
  iota(idx.begin(), idx.end(), 0);

  // when v contains elements of equal values
  stable_sort(idx.begin(), idx.end(),
       [&v](size_t i1, size_t i2) {return v[i1] < v[i2];});

  return idx;
}

static void load_program(const char *filename) {
  assert(filename != NULL);
  program_fd = open(filename, O_RDONLY);
  assert(program_fd != -1);

  struct stat s;
  int ret = fstat(program_fd, &s);
  assert(ret == 0);
  program_sz = s.st_size;

  program = mmap(NULL, program_sz, PROT_READ, MAP_PRIVATE, program_fd, 0);
  assert(program != (void *)-1);

  printf("load program size: 0x%x\n", program_sz);
}

static void close_program() {
  munmap(program, program_sz);
  close(program_fd);
}

void dut_cycle(int n) { while (n --) dut->step(); }
void dut_reset() { dut->set_reset(1); dut_cycle(10); dut->set_reset(0); }

int main(int argc, char** argv) {
  load_program(argv[1]);
  dut = new DUT_NAME();
  memcpy(dut->DUT_MEMORY(), program, program_sz);
  dut_init(dut);
  dut_reset();
  close_program();
  std::cout << "start testing.....\n";
  std::signal(SIGINT, [](int){ dut_end = true; });
  std::signal(SIGTERM, [](int){ dut_end = true; });
  uint64_t cycles = 0;
  auto start = std::chrono::system_clock::now();
  while (!dut_end) {
    dut_cycle(1);
    dut_hook(dut);
    cycles ++;
    if (cycles % (CYCLE_MAX_SIM / (CYCLE_STEP_PERCENT * 100)) == 0 && cycles <= CYCLE_MAX_SIM) {
      auto dur = std::chrono::system_clock::now() - start;
      auto msec = std::chrono::duration_cast<std::chrono::milliseconds>(dur);
      fprintf(stderr, "cycles %ld (%ld ms, %ld per sec) simulation process %.2lf%% \n",
          cycles, msec.count(), cycles * 1000 / msec.count(), (double)cycles * 100 / CYCLE_MAX_SIM);
      if (cycles == CYCLE_MAX_SIM) break;
    }
  }
#if defined(COUNT_ACT)
  fprintf(stderr, "dump act_count.bin\n");
  assert(dut->dump_act_count(COUNT_ACT));
#endif
  return 0;
}
