// Independent C++ typed-stack decision oracle: pinned CE setupCall
// and op_pop_flags_return_val_exit (Fallout 1 CE intrpret.cc).
// This intentionally models synthetic interrupt frames, not real game scripts.
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream in(argv[1]);
    if (!in) return 2;
    std::string label;
    int flags, argc_proc, value, budget;
    while (in >> label >> flags >> argc_proc >> value >> budget) {
        if ((flags & 0x04) != 0 || argc_proc != 0 || budget < 3) {
            std::cout << label << " ERR\n";
            continue;
        }
        // Two return-stack integers, then three saved data-stack slots:
        // flags, null wait callback, window id; setupCall adds zero marker.
        std::vector<int> ret{0, 20};
        std::vector<int> data{0, 0, 0, 0};
        data.pop_back(); // 0x801A POP zero-argument marker
        data.push_back(value);
        const int result = data.back();
        data.pop_back();
        const int window = data.back();
        data.pop_back();
        const int wait_ptr = data.back();
        data.pop_back();
        const int saved_flags = data.back();
        data.pop_back();
        const int return_pc = ret.back();
        ret.pop_back();
        if (window != 0 || wait_ptr != 0 || saved_flags != 0 || return_pc != 20) {
            std::cout << label << " ERR\n";
        } else {
            std::cout << label << " I:" << result << "\n";
        }
    }
}
