#include <immintrin.h>

int main() {
  __m256i_u a = _mm256_set_epi16(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16);
  _mm256_min_epu16(a, a);
  return 0;
}