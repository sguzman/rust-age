// GPL-2.0-or-later. Independent synthetic C++ adaptation of pinned
// Fallout 1 CE intrpret.cc doEvents -> storeLong(flags=0) -> executeProc
// -> setupCall. No compiled CE game script, licensed asset or native engine.
// Rejection of conditional/imported calls and all-or-nothing error rollback
// are intentional bounded Rust adapter gates, not original CE error parity.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string name;
    std::int32_t flags, eventTime, valid;
    std::uint32_t now;
    while (input >> name >> flags >> eventTime >> now >> valid) {
        if (valid != 0 && valid != 1) return 2;
        // Pinned doEvents evaluates CONDITIONAL before TIMED, regardless
        // of the latter's event time. No condition evaluator is installed.
        if ((flags & 0x02) != 0) {
            std::cout << name << " ERR\n";
            continue;
        }
        // CE uses the source event timestamp as UNSIGNED, with strict <.
        if ((flags & 0x01) == 0 ||
            static_cast<std::uint32_t>(eventTime) >= now) {
            std::cout << name << " N:0:0:0:" << flags << ":" << eventTime << ":0\n";
            continue;
        }
        if ((flags & 0x04) != 0 || valid == 0) {
            std::cout << name << " ERR\n";
            continue;
        }
        // doEvents clears all flags; executeProc observes the zeroed
        // procedure entry and only stages the local stack/PC. It does not
        // immediately interpret the action, even if 0x10 was set earlier.
        // setupCall: return PCs [oldPC=0,marker=20], four operand values:
        // saved flags=0, null callback pointer, window ID=0, argument=0.
        std::cout << name << " S:20:4:2:0:" << eventTime << ":0\n";
    }
}
