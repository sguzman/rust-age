// GPL-2.0-or-later. Independent synthetic C++ oracle adapted from pinned
// CE op_map_var/map_get_global_var, op_global_var/game_get_global_var.
// Out-of-range MAP reads return -1. With nonempty game globals,
// out-of-range reads return 0; with no game globals they return -1.
// These are invented fixtures, NOT original compiled Fallout acceptance.
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream stream(argv[1]);
    if (!stream) return 2;
    std::string name;
    int mapCount, gameCount, mapIndex, gameIndex, map0, map1, game0, game1;
    while (stream >> name >> mapCount >> gameCount >> mapIndex >> gameIndex >>
           map0 >> map1 >> game0 >> game1) {
        const int mapValues[] = {map0, map1};
        const int gameValues[] = {game0, game1};
        const int mapResult = mapIndex >= 0 && mapIndex < mapCount
            ? mapValues[mapIndex] : -1;
        const int gameResult = gameCount == 0 ? -1
            : gameIndex >= 0 && gameIndex < gameCount
                ? gameValues[gameIndex] : 0;
        std::cout << name << " S:" << mapResult + gameResult << "\n";
    }
}
