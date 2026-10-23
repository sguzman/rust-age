// GPL-2.0-or-later. Source-derived CE op_tile_contains_obj_pid.
// Scans top-level objects at the requested object-table elevation.
// Invented numeric fixtures only; no original commercial Fallout content.
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream file(argv[1]);
    if (!file) return 2;
    std::string label;
    int qe, qt, qp, e1, t1, p1, e2, t2, p2;
    while (file >> label >> qe >> qt >> qp >> e1 >> t1 >> p1 >> e2 >> t2 >> p2) {
        const int result =
            (e1 == qe && t1 == qt && p1 == qp) ||
            (e2 == qe && t2 == qt && p2 == qp);
        std::cout << label << " S:" << result << " N:5 PC:22\n";
    }
}
