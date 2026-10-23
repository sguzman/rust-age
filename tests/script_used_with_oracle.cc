// GPL-2.0-or-later. Adapted CE op_obj_being_used_with is identical
// to op_target_obj, followed by op_obj_pid's NULL fallback.
// Invented fixtures; not a linked upstream CE executable.
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream file(argv[1]);
    if (!file) return 2;
    std::string label;
    int handle, pid;
    while (file >> label >> handle >> pid) {
        const int result = handle == 0 ? -1 : pid;
        std::cout << label << " S:" << result << " N:3 PC:6\n";
    }
}
