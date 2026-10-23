// GPL-2.0-or-later. Adapted CE action/override C++ oracle, invented data.
// Pinned scripts.cc exec_script_proc and intextra.cc op_script_overrides,
// op_script_action. No game data or original executable.
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

struct Invocation {
    bool present = false;
    int action = 0;
    bool overrides = false;
    void begin(int next) {
        if (present) {
            action = next;
            overrides = false;
        }
    }
};

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string name, kind;
    int present, action;
    while (input >> name >> present >> action >> kind) {
        Invocation current{present != 0, 0, false};
        current.begin(action);
        std::vector<unsigned short> code;
        if (kind == "override" || kind == "override_read") {
            code.push_back(0x80B9);
        }
        if (kind == "read" || kind == "override_read") {
            code.push_back(0x80C7);
        }
        code.push_back(0x8011);
        std::vector<int> stack;
        unsigned pc = 0;
        int steps = 0;
        for (auto op : code) {
            pc += 2;
            ++steps;
            if (op == 0x80B9 && current.present) {
                current.overrides = true;
            } else if (op == 0x80C7) {
                stack.push_back(current.present ? current.action : 0);
            } else if (op == 0x8011) {
                break;
            }
        }
        std::cout << name << " A:" << current.action
                  << " O:" << (current.overrides ? 1 : 0)
                  << " S:" << (stack.empty() ? "E" : std::to_string(stack.back()))
                  << " PC:" << pc << " STEPS:" << steps << "\n";
    }
}
