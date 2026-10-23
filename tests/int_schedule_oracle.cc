// GPL-2.0-or-later. Independent synthetic oracle for CE scheduling metadata.
// Adapted from pinned CE src/int/intrpret.cc op_call_at, op_call_condition,
// op_cancel and op_cancelall; it never loads an original .INT file.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>

struct Procedure {
    int32_t flags;
    int32_t event_time;
    int32_t condition_offset;
};

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream in(argv[1]);
    if (!in) return 2;
    std::string label;
    std::string operation;
    int32_t flags, first, second, now;
    while (in >> label >> operation >> flags >> first >> second >> now) {
        Procedure p{flags, 0, 0};
        if (operation == "at" || operation == "both" || operation == "cancel" || operation == "poll_at") {
            p.event_time = static_cast<int32_t>(
                uint32_t(now) + uint32_t(first) * uint32_t(1000));
            p.flags |= 0x01;
        }
        if (operation == "when" || operation == "both") {
            p.condition_offset = operation == "both" ? second : first;
            p.flags |= 0x02;
        }
        if (operation == "cancel" || operation == "all") {
            p.flags = 0;
            p.event_time = 0;
            p.condition_offset = 0;
        }
        if (operation == "poll_at") {
            // CE doEvents: conditional takes priority and time must be
            // strictly greater than the unsigned scheduled timestamp.
            if ((p.flags & 0x02) == 0 && (p.flags & 0x01) != 0 &&
                uint32_t(p.event_time) < uint32_t(second)) {
                p.flags = 0;
            }
        }
        std::cout << label << " " << p.flags << ","
                  << p.event_time << "," << p.condition_offset << "\n";
    }
}
