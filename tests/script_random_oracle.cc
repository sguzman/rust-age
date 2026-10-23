// GPL-2.0-or-later. Source-adapted synthetic Fallout 1 CE native
// op_random and op_roll_dice oracle with original src/game/roll.cc RNG.
// No commercial game files or original executable.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>

struct CeRng {
    int32_t seed = 0, shuffled = 0, slots[32] = {};
    explicit CeRng(int32_t value) {
        int32_t n = value < 1 ? 1 : value;
        for (int i = 40; i > 0; --i) {
            n = 16807 * (n % 127773) - 2836 * (n / 127773);
            if (n < 0) n &= INT32_MAX;
            if (i < 32) slots[i] = n;
        }
        shuffled = slots[0];
        seed = n;
    }
    int32_t between(int32_t low, int32_t high) {
        const int32_t size = low <= high ? high - low + 1 : low - high + 1;
        int32_t next = 16807 * (seed % 127773) - 2836 * (seed / 127773);
        if (next < 0) next += INT32_MAX;
        if (next < 0) next += INT32_MAX;
        const int slot = shuffled & 31;
        const int32_t previous = slots[slot];
        slots[slot] = next;
        shuffled = previous;
        seed = next;
        const int32_t value = (low <= high ? low : high) + previous % size;
        return value < low || value > high ? low : value;
    }
};
int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label;
    char opcode;
    int seed, low, high, vcr, count;
    while (input >> label >> opcode >> seed >> low >> high >> vcr >> count) {
        CeRng rng(seed);
        std::cout << label << " ";
        for (int i = 0; i < count; ++i) {
            if (i) std::cout << ",";
            // The real native pops high, then low. op_roll_dice remains
            // unimplemented and always pushes integer zero.
            const int result = opcode == 'D' ? 0 : vcr
                ? (high - low) / 2 : rng.between(low, high);
            std::cout << result;
        }
        std::cout << " NEXT:" << rng.between(1, 100) << "\n";
    }
}
