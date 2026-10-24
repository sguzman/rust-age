// GPL-2.0-or-later. Independent invented C++ adaptation of pinned CE
// CALL_AT, CALL_WHEN and CANCEL during updatePrograms interpreter bursts,
// followed by doEvents' unsigned strict timer comparison and setupCall.
// Unsupported conditional execution is a Rust adapter fail-closed rule,
// NOT verified equivalence with original compiled Fallout 1 CE gameplay.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string id, mode;
    std::int32_t seconds;
    std::uint32_t now;
    while (input >> id >> mode >> seconds >> now) {
        if (mode == "WHEN") {
            std::cout << id << " ERR\n";
            continue;
        }
        if (mode == "CANCEL") {
            std::cout << id << " S:2:0:8:0:0:0:0\n";
            continue;
        }
        if (mode != "CALLAT") return 2;
        // CE CALL_AT stores a signed 32-bit clock plus delay*1000 with
        // wrapping arithmetic; doEvents compares stored time as uint32.
        const std::uint32_t eventTimeUnsigned =
            now + static_cast<std::uint32_t>(seconds) * 1000U;
        const std::int32_t eventTime = static_cast<std::int32_t>(eventTimeUnsigned);
        const bool due = eventTimeUnsigned < now;
        const int pc = due ? 20 : 14;
        const int flags = due ? 0 : 1;
        const int retDepth = due ? 2 : 0;
        const int stackDepth = due ? 4 : 0;
        std::cout << id << " S:3:" << static_cast<int>(due)
                  << ":" << pc << ":" << flags << ":" << eventTime
                  << ":" << retDepth << ":" << stackDepth << "\n";
    }
}
