// GPL-2.0-or-later; independent invented CE native bytecode trace.
// Adapted from pinned src/game/scripts.cc game_time_hour/exec_script_proc,
// src/int/support/intextra.cc op_game_time_in_seconds, op_fixed_param,
// op_script_action, op_script_overrides, op_global_var and op_set_global_var.
// No original game assets or executable are included.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

static void word(std::vector<uint8_t>& v, uint16_t n) {
    v.push_back(static_cast<uint8_t>(n >> 8));
    v.push_back(static_cast<uint8_t>(n));
}
static void integer(std::vector<uint8_t>& v, int32_t n) {
    word(v, 0xC001);
    const auto raw = static_cast<uint32_t>(n);
    for (int bits = 24; bits >= 0; bits -= 8) {
        v.push_back(static_cast<uint8_t>(raw >> bits));
    }
}
static std::vector<uint8_t> program() {
    std::vector<uint8_t> code;
    integer(code, 0); // CE setter pops value, then index
    for (uint16_t opcode : {0x80F6, 0x80C7, 0x8039, 0x80F7, 0x8039,
                            0x80EB, 0x8039}) word(code, opcode);
    word(code, 0x80C6);
    word(code, 0x80B9);
    integer(code, 0);
    word(code, 0x80C5);
    word(code, 0x8011);
    return code;
}
static uint16_t read_word(const std::vector<uint8_t>& code, size_t& pc) {
    const auto n = static_cast<uint16_t>((code.at(pc) << 8) | code.at(pc + 1));
    pc += 2;
    return n;
}
static int32_t read_integer(const std::vector<uint8_t>& code, size_t& pc) {
    uint32_t raw = 0;
    for (int i = 0; i < 4; i++) raw = (raw << 8) | code.at(pc++);
    return static_cast<int32_t>(raw);
}
static std::string run(int32_t ticks, int32_t action, int32_t param,
                       bool installed, bool game_loaded) {
    const auto code = program();
    std::vector<int32_t> stack;
    std::vector<int32_t> game = game_loaded ? std::vector<int32_t>{7}
                                             : std::vector<int32_t>{};
    if (!installed) { action = 0; param = 0; }
    bool overrides = false;
    size_t pc = 0;
    int steps = 0;
    bool done = false;
    while (!done && steps < 16) {
        const auto opcode = read_word(code, pc);
        steps++;
        switch (opcode) {
        case 0xC001: stack.push_back(read_integer(code, pc)); break;
        case 0x80F6:
            stack.push_back(100 * ((ticks / 600) / 60 % 24)
                            + (ticks / 600) % 60);
            break;
        case 0x80EB: stack.push_back(ticks / 10); break;
        case 0x80C7: stack.push_back(action); break;
        case 0x80F7: stack.push_back(param); break;
        case 0x8039: {
            int32_t right = stack.back(); stack.pop_back();
            int32_t left = stack.back(); stack.pop_back();
            stack.push_back(left + right);
            break;
        }
        case 0x80C6: {
            int index = stack[stack.size() - 2];
            int value = stack.back();
            stack.pop_back(); stack.pop_back();
            if (!game.empty() && index >= 0 && index < static_cast<int>(game.size())) {
                game[index] = value;
            }
            break;
        }
        case 0x80B9: if (installed) overrides = true; break;
        case 0x80C5: {
            int index = stack.back(); stack.pop_back();
            int value = game.empty() ? -1
                : index < 0 || index >= static_cast<int>(game.size()) ? 0 : game[index];
            stack.push_back(value);
            break;
        }
        case 0x8011: done = true; break;
        default: return "ERR";
        }
    }
    return "S:" + std::to_string(stack.back())
        + " G:" + (game.empty() ? "none" : std::to_string(game[0]))
        + " O:" + std::to_string(overrides ? 1 : 0)
        + " PC:" + std::to_string(pc) + " N:" + std::to_string(steps);
}
int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream in(argv[1]);
    if (!in) return 2;
    std::string label;
    int ticks, action, fixed, present, game;
    while (in >> label >> ticks >> action >> fixed >> present >> game) {
        std::cout << label << " " << run(ticks, action, fixed, present != 0, game != 0) << "\n";
    }
}
