// GPL-2.0-or-later. Adapted CE src/game/object.cc obj_load_func,
// scripts.cc scr_find_obj_from_program, intextra.cc op_self_obj/op_tile_num.
// Unique non-spatial SID fixtures only, no dynamic/ambiguous cases.
// This is a synthetic reference, NOT a linked upstream CE test.
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label;
    int scriptSid, savedOid, objectSid, objectId, tile;
    while (input >> label >> scriptSid >> savedOid >> objectSid >> objectId >> tile) {
        (void)savedOid;  // scr_oid is loaded but not checked by obj_load_func.
        (void)objectId; // Script::owner assignment checks Object::sid.
        const bool hasOwner = objectSid == scriptSid;
        const int nativeTile = hasOwner ? tile : -1; // tile_num(NULL) => -1
        std::cout << label << " S:" << nativeTile << " N:3 PC:6\n";
    }
}
