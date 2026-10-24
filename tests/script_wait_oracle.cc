// Independent invented C++ adaptation of pinned Fallout 1 CE op_wait,
// checkWait and interpret's per-iteration WAIT branch.
// Pinned checkWait returns now <= waitEnd, even though that is an
// unusual wait condition. Rollback, finite budgets, and native rejection
// are deliberately Rust adapter rules, not original CE error semantics.
// No commercial game assets or actual compiled CE engine are used.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label;
    std::string mode;
    std::uint64_t rawStart;
    std::int64_t rawDuration;
    std::uint64_t rawNext;
    int burst;
    while (input >> label >> mode >> rawStart >> rawDuration >> rawNext >> burst) {
        if ((mode != "NOOP" && mode != "BAD")
            || rawStart > UINT32_MAX || rawNext > UINT32_MAX
            || rawDuration < INT32_MIN || rawDuration > INT32_MAX
            || burst < 0) return 2;
        const auto start = static_cast<std::uint32_t>(rawStart);
        const auto duration = static_cast<std::int32_t>(rawDuration);
        const auto now = static_cast<std::uint32_t>(rawNext);
        const auto end = static_cast<std::uint32_t>(
            start + static_cast<std::uint32_t>(duration));
        // First bounded tick executes PUSH_INT and WAIT, installs checkWait.
        std::cout << label << " S:2:8:" << start << ":" << end << "|";
        // The second tick is cloned. Errors must preserve the first tick.
        if (burst > 4096 || (burst > 0 && now <= end && mode == "BAD")) {
            std::cout << "ERR:8:1:0\n";
        } else if (burst == 0 || now > end) {
            // A blocked source WAIT consumes burst iterations, not opcodes.
            std::cout << "S:0:8:1:0\n";
        } else if (burst == 1) {
            // checkWait returns true, clearing WAIT; then executes one NOP.
            std::cout << "S:1:10:0:0\n";
        } else {
            // NOP followed by STOP_PROGRAM.
            std::cout << "S:2:12:0:1\n";
        }
    }
}
