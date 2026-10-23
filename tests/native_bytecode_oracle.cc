// GPL-2.0-or-later. Independent adapted CE variable/core opcode trace.
// Pinned CE src/int/intrpret.cc, src/int/support/intextra.cc, game/map.cc,
// game/game.cc and game/scripts.cc. Invented bytecode only; no game assets.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

struct Value {
    bool pointer = false;
    int32_t raw = 0;
};
void word(std::vector<uint8_t>& out, uint16_t n) {
    out.push_back(static_cast<uint8_t>(n >> 8));
    out.push_back(static_cast<uint8_t>(n));
}
void number(std::vector<uint8_t>& out, int32_t n) {
    out.push_back(static_cast<uint8_t>(static_cast<uint32_t>(n) >> 24));
    out.push_back(static_cast<uint8_t>(static_cast<uint32_t>(n) >> 16));
    out.push_back(static_cast<uint8_t>(static_cast<uint32_t>(n) >> 8));
    out.push_back(static_cast<uint8_t>(n));
}
void push(std::vector<uint8_t>& code, int32_t n, bool ptr = false) {
    word(code, ptr ? 0xE001 : 0xC001);
    number(code, n);
}
std::vector<uint8_t> scenario(const std::string& name) {
    std::vector<uint8_t> c;
    if (name == "map_plus_game") {
        push(c, 1); push(c, 39); word(c, 0x80C4);
        push(c, 1); word(c, 0x80C3);
        push(c, 0); word(c, 0x80C5); word(c, 0x8039);
    } else if (name == "game_set_read") {
        push(c, 1); push(c, 99); word(c, 0x80C6);
        push(c, 1); word(c, 0x80C5);
    } else if (name == "local_pointer") {
        push(c, 0); push(c, 123, true); word(c, 0x80C2);
        push(c, 0); word(c, 0x80C1);
    } else if (name == "map_invalid") {
        push(c, 9); word(c, 0x80C3);
    } else if (name == "local_float") {
        push(c, 1); word(c, 0xA001); number(c, 0x3FC00000);
        word(c, 0x80C2); push(c, 1); word(c, 0x80C1);
    } else if (name == "mixed_add") {
        push(c, 0); word(c, 0x80C3);
        push(c, 1); word(c, 0x80C5); word(c, 0x8039);
    } else if (name == "game_missing") {
        push(c, 0); word(c, 0x80C5);
    }
    word(c, 0x8011);
    return c;
}
uint16_t get16(const std::vector<uint8_t>& bytes, size_t at) {
    return static_cast<uint16_t>((bytes[at] << 8) | bytes[at + 1]);
}
int32_t get32(const std::vector<uint8_t>& bytes, size_t at) {
    uint32_t n = (static_cast<uint32_t>(bytes[at]) << 24) |
        (static_cast<uint32_t>(bytes[at+1]) << 16) |
        (static_cast<uint32_t>(bytes[at+2]) << 8) |
        static_cast<uint32_t>(bytes[at+3]);
    return static_cast<int32_t>(n);
}
std::string repr(const Value& v) {
    return (v.pointer ? "P:" : "I:") + std::to_string(v.raw);
}
std::string result(const std::string& name) {
    auto code = scenario(name);
    std::vector<Value> stack;
    std::vector<int32_t> map{12, -9};
    std::vector<int32_t> game = name == "game_missing"
        ? std::vector<int32_t>{} : std::vector<int32_t>{7, 8};
    std::vector<int32_t> local{10, 11};
    std::vector<int32_t> localPtrs{0, 0};
    size_t pc = 0;
    int steps = 0;
    bool stopped = false;
    while (!stopped && steps < 16) {
        const uint16_t opcode = get16(code, pc);
        pc += 2;
        ++steps;
        if ((opcode & 0x03FF) == 1) {
            const auto n = get32(code, pc);
            pc += 4;
            stack.push_back({opcode == 0xE001, n});
        } else if (opcode == 0x8011) {
            stopped = true;
        } else if (opcode == 0x8039) {
            const int32_t right = stack.back().raw; stack.pop_back();
            const int32_t left = stack.back().raw; stack.pop_back();
            stack.push_back({false, left + right});
        } else if (opcode >= 0x80C1 && opcode <= 0x80C6) {
            const bool setter = (opcode & 1) == 0;
            Value value;
            if (setter) { value = stack.back(); stack.pop_back(); }
            const int index = stack.back().raw; stack.pop_back();
            Value read{false, -1};
            if (opcode == 0x80C1 || opcode == 0x80C2) {
                if (index >= 0 && index < 2) {
                    if (setter) {
                        local[index] = value.pointer ? 0 : value.raw;
                        localPtrs[index] = value.pointer ? value.raw : 0;
                    } else {
                        read = {localPtrs[index] != 0,
                                localPtrs[index] != 0 ? localPtrs[index] : local[index]};
                    }
                }
            } else if (opcode == 0x80C3 || opcode == 0x80C4) {
                if (index >= 0 && index < 2) {
                    if (setter) map[index] = value.pointer ? 0 : value.raw;
                    else read.raw = map[index];
                }
            } else {
                if (!game.empty()) {
                    if (index >= 0 && index < static_cast<int>(game.size())) {
                        if (setter) game[index] = value.raw;
                        else read.raw = game[index];
                    } else if (!setter) read.raw = 0;
                }
            }
            if (!setter) stack.push_back(read);
        } else return "ERR";
    }
    std::string top = stack.empty() ? "E" : repr(stack.back());
    std::string gameText = game.empty() ? "none"
        : std::to_string(game[0]) + "," + std::to_string(game[1]);
    return std::to_string(steps) + " PC:" + std::to_string(pc)
        + " S:" + top
        + " M:" + std::to_string(map[0]) + "," + std::to_string(map[1])
        + " L:" + std::to_string(local[0]) + "," + std::to_string(local[1])
        + " G:" + gameText;
}
int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string name;
    while (input >> name) std::cout << name << " " << result(name) << "\n";
}
