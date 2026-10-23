// GPL-2.0-or-later. Invented independent two-procedure C++ oracle
// adapted from pinned Fallout 1 CE doEvents, IF-independent condition
// stack mutation, op_call_at and timed/conditional flag precedence.
// This is NOT compiled original CE or game-content acceptance.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string name, mode;
    std::int32_t operand, expected;
    std::uint32_t now;
    while (input >> name >> mode >> operand >> expected >> now) {
        const bool carry = mode == "CARRY_FALSE" || mode == "CARRY_TRUE" || mode == "ERROR";
        const bool conditionalSecond = mode == "TIMER_COND";
        if (!carry && mode != "TIMER" && !conditionalSecond) return 2;
        if (mode == "ERROR") {
            std::cout << name << " ERR\n";
            continue;
        }
        int firstFlags = 0x02;
        int secondFlags = (carry || conditionalSecond) ? 0x02 : 0;
        std::int32_t secondTime = 0;
        unsigned mask = 0;
        int stackLength = 0;

        if (carry) {
            // CE doEvents does not clear the operand stack between two
            // condition offsets. First leaves operand after popping truth.
            stackLength = 1;
            if (mode == "CARRY_TRUE") {
                mask |= 1;
                firstFlags = 0;
            }
            // Second EQUAL consumes the persistent first operand.
            --stackLength;
            if (operand == expected) {
                mask |= 2;
                secondFlags = 0;
            }
        } else {
            // CALL_AT reuses the poll timer and signed 32-bit wrapping
            // arithmetic. Even a false condition leaves its schedule write.
            const std::uint32_t event = now
                + static_cast<std::uint32_t>(operand) * 1000u;
            secondTime = static_cast<std::int32_t>(event);
            secondFlags |= 0x01;
            if (!conditionalSecond && event < now) {
                mask |= 2;
                secondFlags = 0;
            }
        }
        std::cout << name << " S:" << mask
                  << ":" << firstFlags << ":" << secondFlags
                  << ":" << secondTime << ":" << stackLength << "\n";
    }
}
