// GPL-2.0-or-later. Independent, invented synthetic state comparison
// adapted from pinned Fallout 1 CE doEvents, A_TO_D, FETCH(_GLOBAL) and
// EQUAL. These cases DO NOT run compiled original CE or Fallout assets.
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream stream(argv[1]);
    if (!stream) return 2;
    std::string name, mode;
    int saved, expected;
    while (stream >> name >> mode >> saved >> expected) {
        std::vector<int> stack;
        std::vector<int> returnStack;
        int base = -1;
        int frame = -1;
        stack.push_back(saved);
        if (mode == "RETURN") {
            returnStack.push_back(stack.back());
            stack.pop_back();
        } else if (mode == "GLOBAL") {
            base = static_cast<int>(stack.size());
            stack.push_back(5);
        } else if (mode == "FRAME") {
            frame = static_cast<int>(stack.size());
            returnStack.push_back(-1);
        } else if (mode != "DATA") {
            return 2;
        }

        // In CE doEvents, condition execution retains interpreter stacks
        // and base/frame pointers. Only the current program flags and PC
        // are temporarily overridden; the evaluated result is popped.
        int value = 0;
        if (mode == "RETURN") {
            if (returnStack.empty()) return 2;
            value = returnStack.back();
        } else if (mode == "GLOBAL") {
            if (base < 1) return 2;
            value = stack.at(static_cast<size_t>(base - 1));
        } else if (mode == "FRAME") {
            if (frame < 1) return 2;
            value = stack.at(static_cast<size_t>(frame - 1));
        } else {
            if (stack.empty()) return 2;
            value = stack.back();
        }
        std::cout << name << " S:" << (value == expected ? 1 : 0) << "\n";
    }
}
