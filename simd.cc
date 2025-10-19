#include <cstdint>
#include <immintrin.h>

template<typename T>
struct Access {
  T value;
  Access(T value) : value(value) {}
  operator T() const { return value; }
};

// struct BlackHole {
//   template <typename T>
//   BlackHole operator = (const T& value) {}
//   template <typename T>
//   BlackHole operator [] (const T& index) { return BlackHole(); }
// };

// template<typename T>
// void clear(T ptr, uint8_t value, size_t size) {
//   memset(ptr, value, size);
// }
// template<>
// void clear(BlackHole * ptr, uint8_t value, size_t size) {}
// template<>
// void clear(BlackHole ptr, uint8_t value, size_t size) {}

int main() {
  __m256i_u a = _mm256_set_epi16(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16);
  _mm256_min_epu16(a, a);
  return 0;
}