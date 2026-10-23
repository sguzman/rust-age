// GPL-2.0-or-later. Synthetic C++ adapter of pinned CE
// src/game/game.cc game_load_info_vars, no copyrighted game data.
#include <cstdlib>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

std::string unescape(const std::string& encoded) {
    std::string result;
    for (size_t i = 0; i < encoded.size(); ++i) {
        if (encoded[i] == '\\' && i + 1 < encoded.size()) {
            const char next = encoded[++i];
            result.push_back(next == 'n' ? '\n' : next == 'r' ? '\r'
                : next == 't' ? '\t' : next);
        } else {
            result.push_back(encoded[i]);
        }
    }
    return result;
}
std::vector<int> parse(const std::string& content) {
    std::istringstream stream(content);
    std::string line;
    std::vector<int> values;
    bool found = false;
    while (std::getline(stream, line)) {
        if (!found) {
            if (line.compare(0, 16, "GAME_GLOBAL_VARS") == 0) found = true;
            continue;
        }
        // CE's fgets loop skips only leading LF or // comments. A
        // Windows CR-only blank line becomes a zero-valued entry.
        if (line.empty() || line.rfind("//", 0) == 0) continue;
        const auto semi = line.find(';');
        if (semi != std::string::npos) line.resize(semi);
        const auto equals = line.find('=');
        int value = 0;
        if (equals != std::string::npos) {
            const char* start = line.c_str() + equals + 1;
            char* end = nullptr;
            long number = std::strtol(start, &end, 10);
            if (end != start) value = static_cast<int>(number);
        }
        values.push_back(value);
    }
    return values;
}
std::string serialize(const std::vector<int>& values) {
    std::string out = std::to_string(values.size()) + ":";
    for (size_t i = 0; i < values.size(); ++i) {
        if (i != 0) out += ",";
        out += std::to_string(values[i]);
    }
    return out;
}
int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream in(argv[1]);
    if (!in) return 2;
    std::string line;
    while (std::getline(in, line)) {
        const auto sep = line.find('|');
        if (sep == std::string::npos) return 2;
        std::cout << line.substr(0, sep) << " "
                  << serialize(parse(unescape(line.substr(sep + 1)))) << "\n";
    }
}
