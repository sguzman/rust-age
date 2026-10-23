// GPL-2.0-or-later, synthetic C++ reference adaptation only.
// Pinned Fallout 1 CE src/int/support/intextra.cc op_{local,map}_var
// and op_set_{local,map}_var; src/game/{scripts,map}.cc variable access.
// No original game assets, CE binary, or native pointer dereferences.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

struct Tagged {
    bool pointer = false;
    int32_t raw = 0;
};
struct State {
    std::vector<int32_t> globals{12, -9};
    std::vector<int32_t> locals{10, 11};
    std::vector<int32_t> globalPointers{0, 0};
    std::vector<int32_t> localPointers{0, 0};
    int local2Offset = -1;
};
std::string repr(Tagged t) {
    return std::string(t.pointer && t.raw != 0 ? "P:" : "I:") + std::to_string(t.raw);
}
std::string array(const std::vector<int32_t>& values) {
    std::string s;
    for (auto n : values) {
        if (!s.empty()) s += ",";
        s += std::to_string(n);
    }
    return s;
}
int localSlot(State& s, int instance, int index) {
    if (instance == 3 || instance == 4 || (instance != 1 && instance != 2)) return -1;
    int count = instance == 1 ? 2 : 3;
    if (index < 0 || index >= count) return -1;
    int offset = instance == 1 ? 0 : s.local2Offset;
    if (offset < 0) {
        offset = static_cast<int>(s.locals.size());
        s.locals.resize(s.locals.size() + count, 0);
        s.localPointers.resize(s.locals.size(), 0);
        s.local2Offset = offset;
    }
    return offset + index;
}
bool read(State& s, bool local, int instance, int index, Tagged& output) {
    int offset = local ? localSlot(s, instance, index) : index;
    auto& values = local ? s.locals : s.globals;
    auto& pointers = local ? s.localPointers : s.globalPointers;
    if (offset < 0 || offset >= static_cast<int>(values.size())) return false;
    output = Tagged{pointers[offset] != 0,
                    pointers[offset] != 0 ? pointers[offset] : values[offset]};
    return true;
}
bool write(State& s, bool local, int instance, int index, Tagged value) {
    int offset = local ? localSlot(s, instance, index) : index;
    auto& values = local ? s.locals : s.globals;
    auto& pointers = local ? s.localPointers : s.globalPointers;
    if (offset < 0 || offset >= static_cast<int>(values.size())) return false;
    if (value.pointer) {
        values[offset] = 0;
        pointers[offset] = value.raw;
    } else {
        values[offset] = value.raw;
        pointers[offset] = 0;
    }
    return true;
}
int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label, opcode, kind;
    int instance, index;
    int32_t raw;
    while (input >> label >> opcode >> instance >> index >> kind >> raw) {
        State state;
        bool local = opcode[0] == 'l';
        bool setter = opcode[1] == 'w';
        Tagged result;
        std::string top = "E", peek = "N";
        if (setter) {
            Tagged incoming{kind == "p", raw};
            bool changed = write(state, local, instance, index, incoming);
            if (changed && read(state, local, instance, index, result)) {
                peek = repr(result);
            }
        } else {
            if (read(state, local, instance, index, result)) {
                top = repr(result);
            } else {
                top = "I:-1";
            }
        }
        std::cout << label << " " << top << " " << peek
                  << " G:" << array(state.globals)
                  << " L:" << array(state.locals) << "\n";
    }
}
