// Independent invented source-adapted CE interpret field_78 oracle.
// CE starts at -1, initializes before the CPU burst loop, and will repeat
// initialization if signed timer wraps to -1. The Rust adapter uses
// all-or-nothing error rollback; original CE longjmp is not modeled.
// This is NOT compiled Fallout CE or original game content.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label, mode;
    std::uint64_t rawFirst, rawNext;
    int firstBurst, nextBurst;
    while (input >> label >> mode >> rawFirst >> firstBurst >> rawNext >> nextBurst) {
        if ((mode != "NOOP" && mode != "BAD")
            || rawFirst > UINT32_MAX || rawNext > UINT32_MAX
            || firstBurst < 0 || nextBurst < 0) return 2;
        int pc = 0;
        std::int32_t entry = -1;
        const std::uint32_t clocks[] = {
            static_cast<std::uint32_t>(rawFirst),
            static_cast<std::uint32_t>(rawNext)
        };
        const int bursts[] = {firstBurst, nextBurst};
        for (int i = 0; i < 2; i++) {
            if (i != 0) std::cout << "|";
            const auto before = entry;
            bool error = bursts[i] > 4096 || (mode == "BAD" && bursts[i] > 0);
            if (!error && entry == -1) entry = static_cast<std::int32_t>(clocks[i]);
            if (!error && mode == "NOOP" && pc == 0 && bursts[i] > 0) {
                pc = 2;
                if (bursts[i] > 1) pc = 4;
            } else if (!error && mode == "NOOP" && pc == 2 && bursts[i] > 0) {
                pc = 4;
            }
            if (error) entry = before;
            std::cout << (error ? "ERR" : "S") << ":" << pc << ":" << entry;
        }
        std::cout << "\n";
    }
}
