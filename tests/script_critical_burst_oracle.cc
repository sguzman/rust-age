// GPL-2.0-or-later. Independent invented C++ adaptation of pinned
// CE src/int/intrpret.cc interpret's critical short-circuit budget.
// Nonoriginal 4096-instruction critical safety bound, native rejection,
// and all-or-nothing failure are explicitly Rust adapter constraints.
// No commercial game code or original compiled CE test asset is used.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

enum class Op { Noop, Start, Done, Stop, Exit, Bad, PushTwo, JumpTwo };

static bool scenario(const std::string& mode, std::vector<Op>& ops, int& pc,
                     bool& critical) {
    pc = 0;
    critical = false;
    if (mode == "PLAIN") {
        ops = {Op::Noop, Op::Noop, Op::Stop};
    } else if (mode == "PAIR") {
        ops = {Op::Start, Op::Done, Op::Noop, Op::Noop, Op::Stop};
    } else if (mode == "SPAN") {
        ops = {Op::Start, Op::Noop, Op::Noop, Op::Done, Op::Noop, Op::Stop};
    } else if (mode == "DEEP") {
        ops = {Op::Start, Op::Noop, Op::Noop, Op::Noop, Op::Noop, Op::Noop,
               Op::Noop, Op::Noop, Op::Done, Op::Noop, Op::Stop};
    } else if (mode == "REENTER") {
        ops = {Op::Start, Op::Done, Op::Start, Op::Done, Op::Noop, Op::Stop};
    } else if (mode == "PRECRIT") {
        ops = {Op::Start, Op::Done, Op::Noop, Op::Noop, Op::Noop, Op::Stop};
        pc = 2; critical = true;
    } else if (mode == "INFINITE" || mode == "PRECRIT_LOOP") {
        ops = {Op::Start, Op::PushTwo, Op::JumpTwo};
        if (mode == "PRECRIT_LOOP") { pc = 2; critical = true; }
    } else if (mode == "BAD_NATIVE") {
        ops = {Op::Start, Op::Bad};
    } else if (mode == "CRIT_STOP") {
        ops = {Op::Start, Op::Stop};
    } else if (mode == "CRIT_EXIT") {
        ops = {Op::Start, Op::Exit};
    } else if (mode == "NOCRIT_BAD") {
        ops = {Op::Bad};
    } else {
        return false;
    }
    return true;
}

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string name, mode;
    int burst;
    while (input >> name >> mode >> burst) {
        std::vector<Op> ops;
        int pc;
        bool critical;
        if (!scenario(mode, ops, pc, critical) || burst < 0) return 2;
        if (burst > 4096) { std::cout << name << " ERR\n"; continue; }
        int left = critical && burst < 3 ? 3 : burst;
        int criticalSteps = 0;
        int steps = 0;
        int stackDepth = 0;
        bool halted = false;
        bool err = false;
        // Interpret while condition: critical || (--a2 != -1).
        // Only normal iterations decrease the CPU burst counter.
        while (!halted) {
            if (critical) {
                if (criticalSteps >= 4096) { err = true; break; }
                ++criticalSteps;
            } else {
                if (left == 0) break;
                --left;
            }
            Op instruction = Op::Bad;
            if (mode == "INFINITE" || mode == "PRECRIT_LOOP") {
                if (pc == 0) instruction = Op::Start;
                else if (pc == 2) instruction = Op::PushTwo;
                else if (pc == 8) instruction = Op::JumpTwo;
            } else if (pc >= 0 && pc % 2 == 0
                       && static_cast<size_t>(pc / 2) < ops.size()) {
                instruction = ops[static_cast<size_t>(pc / 2)];
            }
            switch (instruction) {
                case Op::Start: critical = true; pc += 2; break;
                case Op::Done: critical = false; pc += 2; break;
                case Op::Noop: pc += 2; break;
                case Op::Stop: case Op::Exit: pc += 2; halted = true; break;
                case Op::PushTwo: pc += 6; ++stackDepth; break;
                case Op::JumpTwo: pc = 2; --stackDepth; break;
                case Op::Bad: err = true; break;
            }
            if (err) break;
            ++steps;
        }
        if (critical) err = true; // bounded adapter refuses critical halt
        if (err) std::cout << name << " ERR\n";
        else std::cout << name << " S:" << steps << ":" << pc << ":"
                       << static_cast<int>(critical) << ":"
                       << static_cast<int>(halted) << ":" << stackDepth << "\n";
    }
}
