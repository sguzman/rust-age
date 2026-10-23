// GPL-2.0-or-later. Adapted synthetic CE MAP lifecycle traversal oracle.
// Pinned CE src/game/scripts.cc scr_load_all_scripts and scr_exec_map_*.
// No game files, original executable, or script callbacks are used.
#include <array>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

struct Record {
    int sid;
    bool installed;
    std::array<int, 24> procs;
};
static std::array<Record, 4> records() {
    std::array<Record, 4> r{};
    const int ids[4] = {500, 501, 601, 602};
    for (int i = 0; i < 4; ++i) {
        r[i].sid = ids[i];
        r[i].installed = (i != 2);
        r[i].procs.fill(-1);
    }
    r[0].procs[1] = 1; r[0].procs[23] = 2; r[0].procs[16] = 3;
    r[1].procs[1] = 1; r[1].procs[15] = 2; r[1].procs[23] = 3;
    r[3].procs[1] = 1; r[3].procs[15] = 2;
    return r;
}

static std::string select(const std::string& phase, int map_sid, int flags) {
    auto rows = records();
    int slot = phase == "start" ? 1 : phase == "enter" ? 15 :
        phase == "update" ? 23 : 16;
    std::vector<std::string> result;
    auto add = [&](const Record& r, bool strict, int param) {
        if (!r.installed) return;
        const int proc = r.procs[slot];
        if (proc < 0 || (strict && proc == 0)) return;
        result.push_back(std::to_string(r.sid) + ":" +
            std::to_string(slot) + ":" + std::to_string(proc) +
            ":" + std::to_string(param));
    };
    if (phase == "update" || phase == "exit") {
        for (const Record& r : rows) {
            if (r.sid == map_sid) { add(r, false, -1); break; }
        }
    }
    if (phase != "exit") {
        for (const Record& r : rows) {
            if (phase != "start" && r.sid == map_sid) continue;
            add(r, phase != "start", phase == "enter" ? ((flags & 1) == 0) : -1);
        }
    }
    std::string output;
    for (const auto& r : result) {
        if (!output.empty()) output += ",";
        output += r;
    }
    return output.empty() ? "NONE" : output;
}

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label, phase;
    int sid, flags;
    while (input >> label >> phase >> sid >> flags) {
        std::cout << label << " " << select(phase, sid, flags) << "\n";
    }
}
