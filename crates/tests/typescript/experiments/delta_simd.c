#include <stdint.h>
#include <wasm_simd128.h>

static const uint8_t *read_varint(const uint8_t *input, const uint8_t *end,
                                  unsigned max_bytes, uint8_t last_max,
                                  uint64_t *value) {
  *value = 0;
  for (unsigned i = 0; i < max_bytes && input < end; ++i) {
    uint8_t byte = *input++;
    if (i == max_bytes - 1 && byte > last_max)
      return 0;
    *value |= (uint64_t)(byte & 127) << (7 * i);
    if (byte < 128)
      return input;
  }
  return 0;
}

static void store_prefix(uint32_t *output, v128_t delta, uint32_t *previous) {
  v128_t zero = wasm_i32x4_splat(0);
  delta = wasm_i32x4_add(delta, wasm_i32x4_shuffle(zero, delta, 3, 4, 5, 6));
  delta = wasm_i32x4_add(delta, wasm_i32x4_shuffle(zero, delta, 2, 3, 4, 5));
  delta = wasm_i32x4_add(delta, wasm_i32x4_splat(*previous));
  wasm_v128_store(output, delta);
  *previous = (uint32_t)wasm_i32x4_extract_lane(delta, 3);
}

static inline int32_t decode(const uint8_t *input, uint32_t length,
                             uint32_t *output, uint32_t count, int simd) {
  if (count == 0)
    return 0;
  if (count > length)
    return -1;

  const uint8_t *start = input;
  const uint8_t *end = input + length;
  uint64_t encoded;
  input = read_varint(input, end, 5, 15, &encoded);
  if (!input)
    return -1;
  uint32_t previous = (uint32_t)encoded;
  output[0] = previous;

  for (uint32_t i = 1; i < count;) {
    unsigned scalar_count = 1;
    if (simd && count - i >= 16 && end - input >= 16) {
      v128_t bytes = wasm_v128_load(input);
      unsigned continuation = wasm_i8x16_bitmask(bytes);
      if (continuation == 0) {
        v128_t sign = wasm_i8x16_neg(wasm_v128_and(bytes, wasm_i8x16_splat(1)));
        v128_t delta = wasm_v128_xor(wasm_u8x16_shr(bytes, 1), sign);
        v128_t low = wasm_i16x8_extend_low_i8x16(delta);
        v128_t high = wasm_i16x8_extend_high_i8x16(delta);
        store_prefix(output + i, wasm_i32x4_extend_low_i16x8(low), &previous);
        store_prefix(output + i + 4, wasm_i32x4_extend_high_i16x8(low),
                     &previous);
        store_prefix(output + i + 8, wasm_i32x4_extend_low_i16x8(high),
                     &previous);
        store_prefix(output + i + 12, wasm_i32x4_extend_high_i16x8(high),
                     &previous);
        i += 16;
        input += 16;
        continue;
      }
      // Read up to and including the first multibyte value before probing
      // again.
      scalar_count = (unsigned)__builtin_ctz(continuation) + 1;
    }

    while (scalar_count-- != 0) {
      if (input == end)
        return -1;
      encoded = *input;
      if (encoded < 128)
        ++input;
      else if (!(input = read_varint(input, end, 10, 1, &encoded)))
        return -1;
      previous += (uint32_t)(encoded >> 1) ^ (0u - (uint32_t)(encoded & 1));
      output[i++] = previous;
    }
  }
  return (int32_t)(input - start);
}

__attribute__((export_name("decode_scalar"))) int32_t decode_scalar(
    const uint8_t *input, uint32_t length, uint32_t *output, uint32_t count) {
  return decode(input, length, output, count, 0);
}

__attribute__((export_name("decode_simd"))) int32_t decode_simd(
    const uint8_t *input, uint32_t length, uint32_t *output, uint32_t count) {
  return decode(input, length, output, count, 1);
}
