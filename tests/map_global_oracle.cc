// GPL-2.0-or-later. Synthetic independent CE MAP-global storage oracle.
// Reference: pinned Fallout 1 CE src/game/map.cc map_get/set_global_var.
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label, action;
    int index, value;
    while (input >> label >> action >> index >> value) {
        std::vector<int> globals{12, -9};
        std::vector<int> pointers{0, 0};
        std::string result = "ERR";
        if (index >= 0 && index < static_cast<int>(globals.size())) {
            if (action == "w") {
                globals[index] = value;
                pointers[index] = 0;
            } else if (action == "p") {
                globals[index] = 0;
                pointers[index] = value;
            }
            if (pointers[index] != 0) {
                result = "P:" + std::to_string(pointers[index]);
            } else {
                result = "I:" + std::to_string(globals[index]);
            }
        }
        std::cout << label << " " << result << " "
                  << globals[0] << " " << globals[1] << "\n";
    }
}
