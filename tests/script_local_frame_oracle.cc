// GPL-2.0-or-later. Synthetic source-derived CE executeProc/setupCall
// preflight for a fixed invented .INT with opcode at byte offset 20.
// Rejects imported procedure, arguments and invalid body/opcode markers.
// Not a linked/original C++ interpreter or real script acceptance oracle.
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label;
    int flags, count, body, opcode;
    while (input >> label >> flags >> count >> body >> opcode) {
        const bool valid =
            (flags & 0x04) == 0 && count == 0 && body == 20 &&
            (opcode & 0x8000) != 0;
        std::cout << label;
        if (valid) {
            std::cout << " S:" << body << ",20,0," << ((flags & 0x10) != 0);
        } else {
            std::cout << " ERR";
        }
        std::cout << "\n";
    }
}
