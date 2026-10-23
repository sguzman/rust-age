// GPL-2.0-or-later. Adapted synthetic CE setupCall/return behavior:
// POP_FLAGS_EXIT (0x8021) has no value, POP_FLAGS_RETURN_VAL_EXIT
// (0x8025) preserves the returned integer after stack-frame restoration.
// Invalid or budget-exhausted traces fail. Invented values only, not a
// linked original CE interpreter and not gameplay-script acceptance.
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label, kind;
    int value, budget;
    while (input >> label >> kind >> value >> budget) {
        std::cout << label << " ";
        if (kind == "void" && budget >= 2) {
            std::cout << "S:void";
        } else if (kind == "value" && budget >= 3) {
            std::cout << "S:" << value;
        } else {
            std::cout << "ERR";
        }
        std::cout << "\n";
    }
}
