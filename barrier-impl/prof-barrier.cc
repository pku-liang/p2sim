#include <atomic>
#include <barrier>
#include <numeric>
#include <vector>
#include <thread>
#include <iostream>
#include <x86intrin.h>

#define CACHELINE_SIZE 64

template<typename T>
struct alignas(CACHELINE_SIZE) aligned_atomic {
  std::atomic<T> v {0};
  char pad[CACHELINE_SIZE - sizeof(std::atomic<T>)] {};
  aligned_atomic() = default;
  aligned_atomic(T val) : v(val) {}
  aligned_atomic(const aligned_atomic &r): v(r.v.load()), pad() {}
};

struct at_barrier {
  aligned_atomic<unsigned> arrive;
  aligned_atomic<unsigned> step;
  const int count;
  at_barrier(int count) : count(count), arrive(count), step(0) {}
  void arrive_and_wait(int i, int & local_epoch) noexcept {
    auto cur_step = step.v.load();
    if(!--arrive.v) {
      arrive.v = count;
      ++step.v;
    } else {
      while(step.v.load() == cur_step) {
        _mm_pause();
      }
    }
  }
};

struct sep_barrier {
  std::vector<aligned_atomic<int>> arrive_flags_;
  std::vector<aligned_atomic<int>> wait_flags_;
  sep_barrier(int parties) {
    arrive_flags_.resize(parties);
    wait_flags_.resize(parties);
  }
  void arrive_and_wait(int tid, int & local_epoch) {
    int epoch = local_epoch + 1;
    arrive_flags_[tid].v.store(epoch);
    if(tid == 0) {
      for(auto & flag: arrive_flags_) {
        while(flag.v.load() != epoch) {
          _mm_pause();
        }
      }
      for(auto & flag: wait_flags_) {
        flag.v.store(epoch);
      }
    }
    while(wait_flags_[tid].v.load() != epoch) {
      _mm_pause();
    }
    local_epoch = epoch;
  }
};

struct std_barrier {
  std::barrier<> b;
  std_barrier(int parties) : b(parties) {}
  void arrive_and_wait(int i, int & local_epoch) {
    b.arrive_and_wait();
  }
};

void random_arith(int N) {
  volatile uint64_t sink;
  uint64_t a = 123, b = 456;
  for (size_t i = 0; i < N; i++) {
    a += b;
    b ^= a;
    a = (a << 3) | (a >> 61);
  }
  sink = a + b;
}

template<typename barrier>
uint64_t bench_barrier(int n) {
  std::vector<std::thread> threads;
  barrier b(n);
  int iter = 10000;
  std::atomic<bool> start_flag(false);
  for(int i = 0; i < n; i++) {
    threads.push_back(std::thread([&, i, iter]{
      while(!start_flag) {
        _mm_pause();
      }
      int local_epoch = 0;
      for(int j = 0; j < iter; j++) {
        b.arrive_and_wait(i, local_epoch);
        // random_arith(10000 * i / 2);
        b.arrive_and_wait(i, local_epoch);
        // random_arith(1000);
      }
    }));
  }
  start_flag = true;
  auto start = std::chrono::high_resolution_clock::now();
  for(auto & thread: threads) {
    thread.join();
  }
  auto stop = std::chrono::high_resolution_clock::now();
  auto start_dummy = std::chrono::high_resolution_clock::now();
  // for(int j = 0; j < iter; j++) {
  //   random_arith(10000 * (n - 1) / 2);
  //   random_arith(1000);
  // }
  auto stop_dummy = std::chrono::high_resolution_clock::now();
  auto duration_dummy = std::chrono::duration_cast<std::chrono::nanoseconds>(stop_dummy - start_dummy);
  auto duration = std::chrono::duration_cast<std::chrono::nanoseconds>(stop - start);
  // std::cout << "Bench barrier threads=" << n << " latency=" << (duration.count() - duration_dummy.count()) / iter << " ns" << std::endl;
  return (duration.count() - duration_dummy.count()) / iter;
}

template<typename barrier>
uint64_t bench_barrier(int n, int repeat) {
  std::vector<uint64_t> latencies;
  for(int i = 0; i < repeat; i++) {
    latencies.push_back(bench_barrier<barrier>(n));
  }
  return std::accumulate(latencies.begin(), latencies.end(), 0) / latencies.size();
}

int main() {
  std::cout << "threads\tat\tsep\tstd" << std::endl;
  for(int i = 1; i <= 10; i++) {
    uint64_t at_latency = bench_barrier<at_barrier>(i, 10);
    uint64_t sep_latency = bench_barrier<sep_barrier>(i, 10);
    uint64_t std_latency = bench_barrier<std_barrier>(i, 10);
    std::cout << i << "\t" << at_latency << "\t" << sep_latency << "\t" << std_latency << std::endl;
  }
  return 0;
}
