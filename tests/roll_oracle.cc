// Adapted independent fixed-seed reference from pinned Fallout 1 CE
// src/game/roll.cc and game/scripts.h. Only invented numeric cases.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <string>

struct CeRng {
    int32_t seed = 0;
    int32_t shuffled = 0;
    int32_t slots[32] = {};

    explicit CeRng(int32_t value) { reset(value); }

    void reset(int32_t value) {
        int32_t n = value < 1 ? 1 : value;
        for (int i = 40; i > 0; --i) {
            n = 16807 * (n % 127773) - 2836 * (n / 127773);
            if (n < 0) n &= INT32_MAX;
            if (i < 32) slots[i] = n;
        }
        shuffled = slots[0];
        seed = n;
    }

    int32_t between(int32_t min, int32_t max) {
        const int32_t size = min <= max ? max - min + 1 : min - max + 1;
        int32_t next = 16807 * (seed % 127773) - 2836 * (seed / 127773);
        if (next < 0) next += INT32_MAX;
        if (next < 0) next += INT32_MAX;
        const int index = shuffled & 31;
        const int32_t previous = slots[index];
        slots[index] = next;
        shuffled = previous;
        seed = next;
        int32_t result = (min <= max ? min : max) + previous % size;
        if (result < min || result > max) result = min;
        return result;
    }

    int critical(int delta, int modifier, int ticks) {
        if (delta < 0) {
            if (ticks / 864000 >= 1 && between(1, 100) <= -delta / 10) return 0;
            return 1;
        }
        if (ticks / 864000 >= 1 && between(1, 100) <= delta / 10 + modifier) return 3;
        return 2;
    }
};

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream in(argv[1]);
    if (!in) return 2;
    std::string name, mode;
    int seed;
    while (in >> name >> mode >> seed) {
        CeRng rng(seed);
        std::cout << name << " ";
        if (mode == "S") {
            int min, max, count;
            if (!(in >> min >> max >> count)) return 2;
            for (int i = 0; i < count; ++i) {
                if (i != 0) std::cout << ",";
                std::cout << rng.between(min, max);
            }
        } else if (mode == "C") {
            int difficulty, modifier, ticks;
            if (!(in >> difficulty >> modifier >> ticks)) return 2;
            const int delta = difficulty - rng.between(1, 100);
            std::cout << "D:" << delta << ",R:" << rng.critical(delta, modifier, ticks);
        } else {
            return 2;
        }
        std::cout << "\n";
    }
}
