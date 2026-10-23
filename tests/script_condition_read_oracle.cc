// GPL-2.0-or-later. Independent invented C++ oracle adapted from pinned
// Fallout 1 CE MAP-global bounds/read and integer EQUAL opcode semantics.
// These are synthetic values, not tests against compiled original gameplay.
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream stream(argv[1]);
    if (!stream) return 2;
    std::string name;
    int count, index, wanted, first, second;
    while (stream >> name >> count >> index >> wanted >> first >> second) {
        if (count < 0 || count > 2) return 2;
        const int values[] = {first, second};
        const int mapValue = index >= 0 && index < count
            ? values[index] : -1;
        std::cout << name << " S:" << (mapValue == wanted ? 1 : 0) << "\n";
    }
}
