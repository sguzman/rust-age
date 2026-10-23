// GPL-2.0-or-later. Adapted CE program-list and doEvents traversal oracle.
// Synthetic flags and timestamps only; no real .INT execution.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

struct Entry {
    int32_t flags;
    int32_t timestamp;
    int32_t conditional_result;
};

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream in(argv[1]);
    if (!in) return 2;
    std::string label;
    uint32_t now;
    Entry records[3];
    while (in >> label >> now >>
           records[0].flags >> records[0].timestamp >> records[0].conditional_result >>
           records[1].flags >> records[1].timestamp >> records[1].conditional_result >>
           records[2].flags >> records[2].timestamp >> records[2].conditional_result) {
        // Pinned CE insertProgram prepends, and doEvents scans that list.
        std::vector<int> ready;
        for (int index = 2; index >= 0; --index) {
            const Entry& entry = records[index];
            if (entry.flags & 0x02) {
                if (entry.conditional_result == 1) ready.push_back(index + 1);
            } else if ((entry.flags & 0x01) &&
                       uint32_t(entry.timestamp) < now) {
                ready.push_back(index + 1);
            }
        }
        std::cout << label << " ";
        if (ready.empty()) std::cout << "NONE";
        for (size_t index = 0; index < ready.size(); ++index) {
            if (index) std::cout << ",";
            std::cout << ready[index];
        }
        std::cout << "\n";
    }
}
