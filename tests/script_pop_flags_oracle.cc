// GPL-2.0-or-later. Independent invented source-derived C++ oracle for
// CE op_pop_flags, op_pop_flags_return, op_pop_flags_exit, and
// op_pop_flags_return_val_exit in pinned src/int/intrpret.cc.
// Nonzero host pointers, nonzero windows and unmodeled flags are rejected
// by the BOUNDED RUST ADAPTER, not by original CE; this oracle asserts
// both the supported subset and its explicit fail-closed boundary.
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string name, mode;
    int flags, window, wait, address, value;
    while (input >> name >> mode >> flags >> window >> wait >> address >> value) {
        const bool isFlags = mode == "FLAGS";
        const bool isReturn = mode == "RETURN";
        const bool isExit = mode == "EXIT";
        const bool isValue = mode == "VALUE_EXIT";
        if (!isFlags && !isReturn && !isExit && !isValue) return 2;
        // Safe original-CE subset for no host state. Return target is
        // checked within the synthetic INT program (42+12 bytes).
        if ((flags != 0 && flags != 0x80) || window != 0 || wait != 0 ||
            (!isFlags && (address < 0 || address >= 54))) {
            std::cout << name << " ERR\n";
            continue;
        }
        const int pc = isFlags ? 20 : (isValue ? address : address);
        const int halted = (isExit || isValue) ? 1 : 0;
        const char* reason = halted ? "RETURN" : "NONE";
        std::cout << name << " S:" << pc << ":" << flags
                  << ":" << halted << ":" << reason << ":";
        if (isValue) std::cout << value; else std::cout << "_";
        std::cout << "\n";
    }
}
