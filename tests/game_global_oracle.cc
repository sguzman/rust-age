// GPL-2.0-or-later. Independent synthetic CE game-global C++ oracle.
// Reference: pinned src/game/game.cc game_get_global_var/game_set_global_var
// and src/int/support/intextra.cc op_global_var/op_set_global_var.
// No real user game files or original CE executable are used.
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

std::string values(const std::vector<int>& vars) {
    if (vars.empty()) return "(empty)";
    std::string output;
    for (int var : vars) {
        if (!output.empty()) output += ",";
        output += std::to_string(var);
    }
    return output;
}

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label, operation, initial;
    int index, raw;
    while (input >> label >> operation >> initial >> index >> raw) {
        std::vector<int> globals = initial == "empty"
            ? std::vector<int>{}
            : std::vector<int>{12, -17, 0};
        std::string output;
        if (operation == "r") {
            int result = -1; // op_global_var initializes -1.
            if (!globals.empty()) {
                // game_get_global_var returns zero for invalid indices.
                result = index < 0 || index >= static_cast<int>(globals.size())
                    ? 0 : globals[index];
            }
            output = "I:" + std::to_string(result);
        } else {
            // Native op_set_global_var ignores game_set_global_var's return.
            if (!globals.empty() && index >= 0
                && index < static_cast<int>(globals.size())) {
                globals[index] = raw;
            }
            output = "E";
        }
        std::cout << label << " " << output << " " << values(globals) << "\n";
    }
}
