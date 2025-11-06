#include <immintrin.h>
#include <random>
#include <limits>

struct alignas(32) MH {
  __m256i val[2];
  void reset() {
    val[0] = _mm256_set1_epi16(std::numeric_limits<uint16_t>::max());
    val[1] = _mm256_set1_epi16(std::numeric_limits<uint16_t>::max());
  }
  void update(const MH & data) {
    val[0] = _mm256_min_epu16(val[0], data.val[0]);
    val[1] = _mm256_min_epu16(val[1], data.val[1]);
  }
  static MH from_gen(std::mt19937_64 & rng) {
    union {
      MH res;
      uint64_t val64[4];
    } val;
    val.val64[0] = rng();
    val.val64[1] = rng();
    val.val64[2] = rng();
    val.val64[3] = rng();
    return val.res;
  }
};

int main() {
  std::mt19937_64 rng(0);
  MH mh = MH::from_gen(rng);
  MH mh2 = MH::from_gen(rng);
  mh.update(mh2);
  return 0;
}