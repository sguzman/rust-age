// Independent invented source-adapted CE updatePrograms/doEvents case model.
// Condition takes precedence over the timed flag, uses bounded PUSH/STOP
// and clears flags before local setupCall; its body runs in a later tick.
// Native and imported procedures remain explicitly unsupported; failure
// rolls back the whole Rust adapter transaction (not CE longjmp semantics).
// No original game assets or compiled CE engine are used.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label, mode;
    int flags, result, burst, budget;
    std::uint64_t rawNow;
    while (input >> label >> mode >> flags >> result >> rawNow >> burst >> budget) {
        if ((mode != "GOOD" && mode != "BAD") || rawNow > UINT32_MAX
            || burst < 0 || budget < 0) return 2;
        const auto now = static_cast<std::uint32_t>(rawNow);
        const bool condition = (flags & 2) != 0;
        const bool timed = (flags & 1) != 0 && now > 10;
        const bool shouldPrepare = condition ? result != 0 : timed;
        const bool error = burst > 4096
            || (condition && (mode == "BAD" || budget < 2))
            || (shouldPrepare && (flags & 4) != 0);
        std::cout << label << " ";
        if (error) {
            std::cout << "ERR:0:" << flags << ":0:0:-1:0\n";
        } else {
            const int steps = burst > 2 ? 2 : burst;
            const int pc = shouldPrepare ? 36 : steps * 2;
            const int newFlags = shouldPrepare ? 0 : flags;
            const int stack = shouldPrepare ? 4 : 0;
            const int returns = shouldPrepare ? 2 : 0;
            const auto started = static_cast<std::int32_t>(now);
            std::cout << "S:" << steps << ":" << pc << ":" << newFlags
                      << ":" << stack << ":" << returns << ":" << started
                      << ":" << static_cast<int>(shouldPrepare) << "\n";
        }
    }
}
