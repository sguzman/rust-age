// GPL-2.0-or-later. Independent invented C++ oracle for CE interpreter
// op_exit/op_exit_prog -> PROGRAM_FLAG_EXITED and
// op_stop_prog -> PROGRAM_FLAG_STOPPED, plus doEvents integer condition truth.
// Based on pinned Fallout 1 CE intrpret.cc, not compiled game execution.
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string name, mode;
    int value;
    while (input >> name >> mode >> value) {
        const char* kind;
        int terminalFlag;
        if (mode == "EXIT") {
            kind = "Exit";
            terminalFlag = 0x01;
        } else if (mode == "EXIT_PROGRAM") {
            kind = "ExitProgram";
            terminalFlag = 0x01;
        } else if (mode == "STOP_PROGRAM") {
            kind = "StopProgram";
            terminalFlag = 0x08;
        } else {
            return 2;
        }
        std::cout << name << " S:" << (value != 0 ? 1 : 0)
                  << ":" << terminalFlag << ":" << kind << "\n";
    }
}
