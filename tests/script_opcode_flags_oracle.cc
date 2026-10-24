// Independent invented adaptation of pinned CE intrpret.cc:
//   program->flags = (program->flags & 0xFFFF) | (opcode << 16)
// Low flags modeled ONLY for EXITED, STOPPED, WAITING and CRITICAL.
// The bounded Rust adapter rolls back rejected opcodes and budgets;
// this deliberately differs from original CE longjmp behavior.
// No commercial game scripts, compiled CE, or host callbacks.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>

static std::uint32_t flags(std::uint16_t opcode, std::uint32_t low) {
    return (static_cast<std::uint32_t>(opcode) << 16) | low;
}

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream in(argv[1]);
    if (!in) return 2;
    std::string label, mode;
    std::uint64_t rawStart, rawNow;
    std::int64_t rawDelay;
    int burst;
    while (in >> label >> mode >> rawStart >> rawDelay >> rawNow >> burst) {
        if (rawStart > UINT32_MAX || rawNow > UINT32_MAX
            || rawDelay < INT32_MIN || rawDelay > INT32_MAX || burst < 0)
            return 2;
        const auto start = static_cast<std::uint32_t>(rawStart);
        const auto now = static_cast<std::uint32_t>(rawNow);
        const auto delay = static_cast<std::int32_t>(rawDelay);
        int firstSteps = 1, firstPc = 2;
        std::uint32_t firstFlags = flags(0x8000, 0);
        bool halted = false;
        if (mode == "CRIT") {
            // CE's critical short-circuit advances LEAVE even after a
            // one-instruction normal burst runs out.
            firstSteps = 2;
            firstPc = 4;
            firstFlags = flags(0x8003, 0);
        } else if (mode == "STOP") {
            firstFlags = flags(0x8011, 8);
            halted = true;
        } else if (mode == "EXIT") {
            firstFlags = flags(0x800E, 1);
            halted = true;
        } else if (mode == "WAIT") {
            firstSteps = 2;
            firstPc = 8;
            firstFlags = flags(0x8047, 16);
        } else if (mode != "PLAIN" && mode != "BAD") {
            return 2;
        }
        std::cout << label << " S:" << firstSteps << ":" << firstPc
                  << ":" << firstFlags << "|";
        if (burst > 4096 || (mode == "BAD" && burst > 0)) {
            std::cout << "ERR:" << firstPc << ":" << firstFlags << "\n";
            continue;
        }
        int secondSteps = 0, secondPc = firstPc;
        std::uint32_t secondFlags = firstFlags;
        if (burst > 0 && !halted) {
            if (mode == "PLAIN") {
                secondSteps = burst >= 2 ? 2 : 1;
                secondPc = burst >= 2 ? 6 : 4;
                secondFlags = burst >= 2 ? flags(0x8011, 8) : flags(0x8000, 0);
            } else if (mode == "CRIT") {
                secondSteps = 1;
                secondPc = 6;
                secondFlags = flags(0x8011, 8);
            } else if (mode == "WAIT") {
                const auto end = static_cast<std::uint32_t>(
                    start + static_cast<std::uint32_t>(delay));
                if (now <= end) {
                    secondSteps = burst >= 2 ? 2 : 1;
                    secondPc = burst >= 2 ? 12 : 10;
                    secondFlags = burst >= 2 ? flags(0x8011, 8) : flags(0x8000, 0);
                }
            }
        }
        std::cout << "S:" << secondSteps << ":" << secondPc
                  << ":" << secondFlags << "\n";
    }
}
