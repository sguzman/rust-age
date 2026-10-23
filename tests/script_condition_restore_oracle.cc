// GPL-2.0-or-later. Invented, independent C++ reference for pinned
// Fallout 1 CE intrpret.cc doEvents' successful-condition save/zero/
// restore protocol, op_critical_start, and distinct EXIT/STOP flags.
// Adapted from upstream source, not compiled original CE or game assets.
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream stream(argv[1]);
    if (!stream) return 2;
    std::string name, initial, terminal, critical;
    int truth;
    while (stream >> name >> initial >> terminal >> critical >> truth) {
        int savedPc = 0;
        int savedFlags = 0;
        if (initial == "CRITICAL") {
            savedPc = 2;
            savedFlags = 0x80;
        } else if (initial == "STOP") {
            savedPc = 2;
            savedFlags = 0x08;
        } else if (initial == "CRITICAL_STOP") {
            savedPc = 4;
            savedFlags = 0x88;
        } else if (initial != "NONE") {
            return 2;
        }
        int evaluatedFlags = 0;
        if (terminal == "STOP_PROGRAM") {
            evaluatedFlags = 0x08;
        } else if (terminal == "EXIT" || terminal == "EXIT_PROGRAM") {
            evaluatedFlags = 0x01;
        } else {
            return 2;
        }
        if (critical == "1") {
            evaluatedFlags |= 0x80;
        } else if (critical != "0") {
            return 2;
        }
        const int evaluatedPc = critical == "1" ? 30 : 28;
        // A successful doEvents condition restores the original flags
        // and instruction pointer. It does NOT restore operand stacks.
        std::cout << name << " S:" << (truth != 0 ? 1 : 0)
                  << ":" << savedPc << ":" << evaluatedPc << ":" << savedPc
                  << ":" << savedFlags << ":" << evaluatedFlags
                  << ":" << savedFlags << "\n";
    }
}
