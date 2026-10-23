// GPL-2.0-or-later. Synthetic CE typed-string comparison oracle.
// Adapted from pinned Fallout 1 CE src/int/intrpret.cc op_equal,
// op_not_equal and order comparisons; no game files or original script code.
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iostream>
#include <string>

struct Value {
    char kind;
    std::string text;
};

static std::string ce_string(Value v) {
    if (v.kind == 'S') return v.text;
    if (v.kind == 'I') return std::to_string(std::stoi(v.text));
    if (v.kind == 'F') {
        char formatted[80];
        std::snprintf(formatted, sizeof(formatted), "%.5f", std::stof(v.text));
        return formatted;
    }
    throw std::runtime_error("invalid string operand");
}

static int comparison(Value left, Value right) {
    if (left.kind == 'S' || right.kind == 'S') {
        const std::string a = ce_string(left), b = ce_string(right);
        return std::strcmp(a.c_str(), b.c_str());
    }
    if (left.kind == 'P' || right.kind == 'P') {
        const int32_t a = std::stoi(left.text), b = std::stoi(right.text);
        return (a > b) - (a < b);
    }
    const float a = std::stof(left.text), b = std::stof(right.text);
    return (a > b) - (a < b);
}

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label, left, right, op;
    char left_kind, right_kind;
    while (input >> label >> left_kind >> left >> right_kind >> right >> op) {
        try {
            const int c = comparison({left_kind, left}, {right_kind, right});
            bool result;
            if (op == "eq") result = c == 0;
            else if (op == "ne") result = c != 0;
            else if (op == "lt") result = c < 0;
            else if (op == "le") result = c <= 0;
            else if (op == "gt") result = c > 0;
            else if (op == "ge") result = c >= 0;
            else throw std::runtime_error("invalid comparison");
            std::cout << label << " I:" << (result ? 1 : 0) << "\n";
        } catch (const std::exception&) {
            std::cout << label << " ERR\n";
        }
    }
}
