// GPL-2.0-or-later, independent synthetic CE integer local-array oracle.
// Pinned CE scripts.cc scr_get/set_local_var, map.cc map_malloc_local_var.
// Bounded Rust adapter intentionally disallows system-script access.
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

struct Slot {
    int category, declared, offset;
};
static Slot binding(int instance) {
    switch (instance) {
    case 1: return {1, 2, 0};
    case 2: return {1, 3, -1};
    case 3: return {0, 2, -1};
    case 4: return {1, 0, -1};
    default: return {-1, 0, -1};
    }
}
int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream in(argv[1]);
    if (!in) return 2;
    std::string label, op;
    int instance, variable, value;
    while (in >> label >> op >> instance >> variable >> value) {
        std::vector<int> locals{10, 11};
        Slot slot = binding(instance);
        std::string output = "ERR";
        if (slot.category > 0 && slot.declared == 0 && op == "r") {
            output = "NONE";
        } else if (slot.category > 0 && slot.declared > 0 &&
                   variable >= 0 && variable < slot.declared) {
            if (slot.offset < 0) {
                slot.offset = static_cast<int>(locals.size());
                locals.resize(locals.size() + slot.declared, 0);
            }
            const int index = slot.offset + variable;
            if (op == "w") locals[index] = value;
            output = "I:" + std::to_string(locals[index]);
        }
        std::cout << label << " " << output << " "
                  << locals.size() << " " << slot.offset << "\n";
    }
}
