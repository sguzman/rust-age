// GPL-2.0-or-later. Independent invented C++ reference adapted from
// pinned Fallout 1 CE updatePrograms: interpret newest-first, doEvents
// after interpreter bursts. This only asserts supported core-only behavior.
// Conditional/imported/native failures and atomic rollback are bounded Rust
// adapter constraints, not full original CE gameplay/error equivalence.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string id;
    std::int32_t flags, time, burst, bad;
    std::uint32_t now;
    while (input >> id >> flags >> time >> now >> burst >> bad) {
        if (burst < 0 || burst > 3 || (bad != 0 && bad != 1)) return 2;
        // Both invented programs have NOP, NOP, STOP at PC 0/2/4.
        // Source order runs program ID 2 first, then ID 1.
        if (bad != 0 && burst != 0) {
            std::cout << id << " ERR\n";
            continue;
        }
        // Timed-only bounded doEvents refuses conditions instead of
        // claiming to have evaluated the game script's condition.
        if ((flags & 0x02) != 0) {
            std::cout << id << " ERR\n";
            continue;
        }
        const bool due = (flags & 0x01) != 0 &&
                         static_cast<std::uint32_t>(time) < now;
        // Imported local procedure host unavailable. Pinned CE
        // setupCall saves and clears the STOPPED low flag, so a script
        // whose main program STOPPED may still receive an interrupt;
        // op_pop_flags restores STOPPED after its later local return.
        if (due && (flags & 0x04) != 0) {
            std::cout << id << " ERR\n";
            continue;
        }
        const int prepared = due ? 1 : 0;
        const int pc2 = burst * 2;
        const int pc1 = due ? 20 : burst * 2;
        const int finalFlags = due ? 0 : flags;
        const int returnDepth = due ? 2 : 0;
        const int halted = !due && burst == 3 ? 1 : 0;
        std::cout << id << " S:" << prepared << ":" << burst
                  << ":" << burst << ":" << pc2 << ":" << pc1
                  << ":" << finalFlags << ":" << returnDepth
                  << ":" << halted << "\n";
    }
}
