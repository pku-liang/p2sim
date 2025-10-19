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
#include "TestHarness.h"
#include <fesvr/dtm.h>

#define CYCLE_STEP_PERCENT 1

static TestHarness * dut = nullptr;
static dtm_t * dtm = nullptr;

void dut_cycle(int n) { while (n --) dut->step(); }
void dut_reset() { dut->set_reset(1); dut_cycle(10); dut->set_reset(0); }

/*
input reset : UInt<1>
output debug : {
  req : {
    flip ready : UInt<1>,
    valid : UInt<1>,
    bits : {
      addr : UInt<7>,
      data : UInt<32>,
      op : UInt<2>
    }
  },
  flip resp : {
    flip ready : UInt<1>,
    valid : UInt<1>,
    bits : {
      data : UInt<32>,
      resp : UInt<2>
    }
  }
}
output exit : UInt<32>    
*/

void SimDTM(
  uint8_t reset,
  uint8_t debug_req_ready,
  uint8_t& debug_req_valid,
  uint8_t& debug_req_bits_addr,
  uint32_t& debug_req_bits_data,
  uint8_t& debug_req_bits_op,
  uint8_t& debug_resp_ready,
  uint8_t debug_resp_valid,
  uint32_t debug_resp_bits_data,
  uint8_t debug_resp_bits_resp,
  uint32_t& exit
) {

  if(!reset) [[likely]] {
    dtm_t::resp resp_bits;
    resp_bits.resp = debug_resp_bits_resp;
    resp_bits.data = debug_resp_bits_data;

    dtm->tick
    (
      debug_req_ready,
      debug_resp_valid,
      resp_bits
    );

    debug_resp_ready = dtm->resp_ready();
    debug_req_valid = dtm->req_valid();
    debug_req_bits_addr = dtm->req_bits().addr;
    debug_req_bits_op = dtm->req_bits().op;
    debug_req_bits_data = dtm->req_bits().data;
    exit = dtm->done() ? (dtm->exit_code() << 1 | 1) : 0;
  } else {
    debug_resp_ready = 0;
    debug_req_valid = 0;
    debug_req_bits_addr = 0;
    debug_req_bits_op = 0;
    debug_req_bits_data = 0;
    exit = 0;
  }
}

int main(int argc, char** argv) {
  dtm = new dtm_t(argc, argv);
  dut = new TestHarness();
  dut->init();
  dut->set_reset(1);
  dut_cycle(10);
  dut->set_reset(0);
  dut_cycle(1);
  int cycle = 0;
  int max_cycles = std::numeric_limits<int>::max();
  if(getenv("MAX_CYCLES")) {
    max_cycles = std::atoi(getenv("MAX_CYCLES"));
  }
  #ifdef PROFILE
  if(!getenv("ACT_INFO_FILE")) {
    printf("ACT_INFO_FILE is not set, please set it to the file to dump act info\n");
  }
  #endif
  clock_t prev = clock();
  while (true) {
    cycle++;
    dut_cycle(1);
    if(cycle % 100000 == 0) {
      clock_t time = clock();
      printf("cycle=%d, freq=%lf kHz\n", cycle, cycle * 1.0 / (time - prev) * CLOCKS_PER_SEC / 1000);
    }
    if(cycle >= 10 && dut->get_io$$success()) {
      break;
    }
    if(cycle >= max_cycles) {
      printf("cycle=%d, freq=%lf kHz\n", cycle, cycle * 1.0 / (clock() - prev) * CLOCKS_PER_SEC / 1000);
      printf("cycle reached max cycles, this may be a bug\n");
      exit(1);
    }
  }
#ifdef PROFILE
  if(getenv("ACT_INFO_FILE")) {
    dut->dump_act_info(getenv("ACT_INFO_FILE"));
  }
#endif
  printf("cycle=%d, freq=%lf kHz\n", cycle, cycle * 1.0 / (clock() - prev) * CLOCKS_PER_SEC / 1000);
  exit(0);
}
