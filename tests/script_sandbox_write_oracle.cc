// GPL-2.0-or-later. Synthetic source-derived CE op_set_map_var,
// op_set_global_var and op_script_overrides. Out-of-range writes are ignored.
// Values are invented. This is NOT the original compiled CE interpreter.
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream stream(argv[1]);
    if (!stream) return 2;
    std::string label;
    int target, mapCount, gameCount, index, value, map0, map1, game0, game1, overrides;
    while (stream >> label >> target >> mapCount >> gameCount >> index >> value >>
           map0 >> map1 >> game0 >> game1 >> overrides) {
        int newMap[] = {map0, map1};
        int newGame[] = {game0, game1};
        if (target == 0 && index >= 0 && index < mapCount) {
            newMap[index] = value;
        }
        if (target == 1 && index >= 0 && index < gameCount) {
            newGame[index] = value;
        }
        const int mapDelta = (newMap[0] != map0) + (newMap[1] != map1);
        const int gameDelta = (newGame[0] != game0) + (newGame[1] != game1);
        std::cout << label << " M:" << newMap[0] << "," << newMap[1]
                  << " G:" << newGame[0] << "," << newGame[1]
                  << " D:" << mapDelta << "," << gameDelta
                  << " O:" << overrides << "\n";
    }
}
