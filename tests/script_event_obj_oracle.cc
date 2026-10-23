// GPL-2.0-or-later. Adapted CE op_source_obj, op_target_obj and
// scr_set_objs: explicit pointers supplied per synthetic script event.
// Source/target NULL fallbacks flow into tile_num/obj_pid (-1).
// Not a linked CE interpreter oracle; test values are invented.
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label;
    int sourceHandle, targetHandle, sourceTile, targetPid;
    while (input >> label >> sourceHandle >> targetHandle >> sourceTile >> targetPid) {
        const int tile = sourceHandle == 0 ? -1 : sourceTile;
        const int pid = targetHandle == 0 ? -1 : targetPid;
        std::cout << label << " S:" << tile + pid << " N:6 PC:12\n";
    }
}
