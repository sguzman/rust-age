// Adapted independent test oracle for pinned Fallout 1 CE
// src/int/intrpret.cc op_lookup_string_proc. All inputs are invented.
#include <cctype>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

static std::vector<std::string> split(const std::string& names) {
    if (names == "-") return {};
    std::vector<std::string> result;
    std::stringstream input(names);
    std::string name;
    while (std::getline(input, name, ',')) result.push_back(name);
    return result;
}

static bool same_name(const std::string& left, const std::string& right) {
    if (left.size() != right.size()) return false;
    for (size_t i = 0; i < left.size(); ++i) {
        const auto a = static_cast<unsigned char>(left[i]);
        const auto b = static_cast<unsigned char>(right[i]);
        if (std::tolower(a) != std::tolower(b)) return false;
    }
    return true;
}

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label;
    std::string names;
    std::string query;
    while (input >> label >> names >> query) {
        const auto procedures = split(names);
        int match = -1;
        // CE op_lookup_string_proc excludes the main procedure (index 0).
        for (size_t index = 1; index < procedures.size(); ++index) {
            if (same_name(procedures[index], query)) {
                match = static_cast<int>(index);
                break;
            }
        }
        std::cout << label << " ";
        if (match < 0) std::cout << "ERR\n";
        else std::cout << "I:" << match << "\n";
    }
}
