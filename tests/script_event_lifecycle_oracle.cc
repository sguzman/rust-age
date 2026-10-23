// GPL-2.0-or-later. Adapted from pinned CE scripts.cc
// scr_set_objs and exec_script_proc, non-spatial owner cases only.
// This synthetic reference models entry, normal completion and reentry;
// it is NOT linked to the original C++ engine or real Fallout assets.
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream stream(argv[1]);
    if (!stream) return 2;
    std::string label;
    int source, target, owner, fixed, action;
    while (stream >> label >> source >> target >> owner >> fixed >> action) {
        // scr_set_objs(sid, source, target).
        // exec_script_proc uses the existing fixedParam, overwrites action,
        // and when target is NULL, assigns script->owner.
        if (target == 0) target = owner;
        const int first_source = source;
        const int first_target = target;
        // A normal exec_script_proc completion clears source only.
        source = 0;
        // Reentry changes action without clearing fixedParam or target.
        std::cout << label
                  << " A:" << first_source << "," << first_target << "," << action << "," << fixed
                  << " B:" << source << "," << target << ",23," << fixed << "\n";
    }
}
