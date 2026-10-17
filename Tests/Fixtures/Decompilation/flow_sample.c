// Original test fixture; released into the public domain.
#include <stddef.h>

__attribute__((noinline)) int add_values(int left, int right) {
    return left + right;
}

__attribute__((noinline)) int classify_and_sum(const int *values, size_t count, int cutoff) {
    int total = 0;
    for (size_t index = 0; index < count; ++index) {
        const int value = values[index];
        if (value < cutoff) {
            total += value;
        } else {
            switch (value & 3) {
                case 0: total += value; break;
                case 1: total -= value; break;
                case 2: total ^= value; break;
                default: total |= value; break;
            }
        }
    }
    return total;
}

int main(void) {
    const int values[] = { 3, 7, 12, 19, 24, 31 };
    return classify_and_sum(values, sizeof(values) / sizeof(values[0]), 10) + add_values(2, 3);
}
