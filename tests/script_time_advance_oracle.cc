// GPL-2.0-or-later. Source-derived, synthetic CE time-advance loop oracle.
// Pinned fallout1-ce 0609bcf: src/int/support/intextra.cc
// op_game_time_advance. This records increments/queue_process calls only.
// No game assets, live queue callbacks, or time mutation.
#include <fstream>
#include <iostream>
#include <string>

static constexpr int kTicksPerDay = 864000;

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label;
    int ticks;
    int maxPolls;
    while (input >> label >> ticks >> maxPolls) {
        const int days = ticks / kTicksPerDay;
        const int fullDays = days > 0 ? days : 0;
        const int requiredPolls = fullDays + 1;
        if (requiredPolls > maxPolls) {
            std::cout << label << " ERR\n";
            continue;
        }
        std::cout << label << " Q:" << requiredPolls << " D:";
        for (int day = 0; day < fullDays; ++day) {
            if (day != 0) std::cout << ',';
            std::cout << kTicksPerDay;
        }
        if (fullDays != 0) std::cout << ',';
        std::cout << (ticks % kTicksPerDay) << '\n';
    }
}
