// GPL-2.0-or-later. Invented, independent C++ source-adapted
// CE executeProc -> setupCall -> CALL_AT/CANCEL -> POP_FLAGS_EXIT/VALUE.
// The Rust adapter's atomic failure rule is an extra safety gate; original
// CE's longjmp/error effects are NOT asserted equivalent to these cases.
// No compiled CE engine or Fallout proprietary asset is used.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream cases(argv[1]);
    if (!cases) return 2;
    std::string id, mode;
    std::int32_t value, delay, now, flags, budget;
    while (cases >> id >> mode >> value >> delay >> now >> flags >> budget) {
        const bool returnsValue = mode == "VALUE" || mode == "VALUE_TIMER";
        const bool timer = mode == "TIMER" || mode == "VALUE_TIMER" ||
                           mode == "FAIL_NATIVE" || mode == "FAIL_BUDGET";
        const bool cancel = mode == "CANCEL";
        const bool voidAction = mode == "VOID";
        if (!returnsValue && !timer && !cancel && !voidAction) return 2;
        int steps = 2; // POP setupCall argument, POP_FLAGS_EXIT
        if (returnsValue) ++steps;
        if (timer) steps += 3; // PUSH seconds, PUSH index, CALL_AT
        if (cancel) steps += 2; // PUSH index, CANCEL
        if (mode == "FAIL_NATIVE") ++steps;
        // Fail before POP_FLAGS exit on budget exhaustion, or an
        // unsupported gameplay opcode. Staged state is discarded.
        if (mode == "FAIL_NATIVE" || budget < steps) {
            std::cout << id << " ERR\n";
            continue;
        }
        std::int32_t eventTime = 77;
        std::int32_t condition = 28;
        if (timer) {
            const std::uint32_t newTime =
                static_cast<std::uint32_t>(now) +
                static_cast<std::uint32_t>(delay) * 1000U;
            eventTime = static_cast<std::int32_t>(newTime);
            flags |= 0x01;
        }
        if (cancel) {
            flags = 0;
            eventTime = 0;
            condition = 0;
        }
        std::cout << id << " S:";
        if (returnsValue) std::cout << value; else std::cout << "_";
        std::cout << ":20:" << (returnsValue ? 1 : 0)
                  << ":1:" << flags << ":" << eventTime
                  << ":" << condition << ":0\n";
    }
}
