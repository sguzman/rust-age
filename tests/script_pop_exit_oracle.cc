// GPL-2.0-or-later. Independent invented source-derived C++ oracle for
// pinned Fallout 1 CE op_pop_exit and interpret's flag 0x40 cleanup.
// Does not run compiled original CE or access any commercial game data.
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream in(argv[1]);
    if (!in) return 2;
    std::string name, mode;
    int destination, value, critical;
    while (in >> name >> mode >> destination >> value >> critical) {
        if ((mode != "RUN" && mode != "CONDITION") || (critical != 0 && critical != 1)) {
            return 2;
        }
        // CE op_pop_exit takes the return stack's saved instruction
        // pointer, raises PROGRAM_FLAG_0x40, and interpret clears that
        // transient flag before the original caller observes it.
        const int observedLow = critical != 0 ? 0x80 : 0;
        const int restoredLow = mode == "CONDITION" ? 0 : observedLow;
        // During a CE condition only, doEvents additionally pops truth.
        const int stackLen = mode == "CONDITION" ? 0 : 1;
        std::cout << name << " S:" << destination << ":" << observedLow
                  << ":" << restoredLow << ":" << stackLen << ":"
                  << (value != 0 ? 1 : 0) << "\n";
    }
}
