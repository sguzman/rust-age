// GPL-2.0-or-later. Independent source-derived Fallout 1 CE object
// property stack oracle. Pinned intextra.cc op_tile_num, op_elevation,
// op_obj_pid and op_obj_art_fid. Fixtures are invented Object snapshots;
// no C++ engine pointers or proprietary game data are used.
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream in(argv[1]);
    if (!in) return 2;
    std::string label;
    char kind;
    int handle;
    int elevation;
    int tile;
    int pid;
    int fid;
    while (in >> label >> kind >> handle >> elevation >> tile >> pid >> fid) {
        int result;
        switch (kind) {
        case 't': result = handle == 0 ? -1 : tile; break;
        case 'e': result = handle == 0 ? 0 : elevation; break;
        case 'p': result = handle == 0 ? -1 : pid; break;
        case 'f': result = handle == 0 ? 0 : fid; break;
        default: return 2;
        }
        // Synthetic INT: PUSH_PTR (six bytes), native (two), STOP (two).
        std::cout << label << " S:" << result << " N:3 PC:10\n";
    }
}
