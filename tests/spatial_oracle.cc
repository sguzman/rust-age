// GPL-2.0-or-later: original numeric tile formulas adapted from the pinned
// Fallout 1 Community Edition src/game/tile.cc functions tile_coord,
// tile_set_center and square_coord. No original game data included.
// This independent C++ reference checks Rust's integer division behavior.
#include <iostream>

int main() {
    for (int center : {20099, 20098, 20100, 30000, 1, 39998}) {
        int tile_x = 199 - center % 200;
        const int tile_y = center / 200;
        int tile_offx = (640 - 32) / 2;
        const int tile_offy = (380 - 16) / 2;
        if (tile_x & 1) {
            tile_x--;
            tile_offx -= 32;
        }
        const int square_x = tile_x / 2;
        const int square_y = tile_y / 2;
        int square_offx = tile_offx - 16;
        int square_offy = tile_offy - 2;
        if (tile_y & 1) {
            square_offx -= 16;
            square_offy -= 12;
        }

        for (int tile = 0; tile < 40000; tile += 137) {
            const int column = 199 - tile % 200;
            const int row = tile / 200;
            const int delta = column - tile_x;
            int screen_x = tile_offx + 48 * (delta / 2);
            int screen_y = tile_offy + 12 * (delta / -2);
            if (column & 1) {
                if (column <= tile_x) {
                    screen_x -= 16;
                    screen_y += 12;
                } else {
                    screen_x += 32;
                }
            }
            const int vertical = row - tile_y;
            screen_x += 16 * vertical;
            screen_y += 12 * vertical;
            std::cout << "T " << center << " " << tile << " " << screen_x
                      << " " << screen_y << "\n";
        }
        for (int square = 0; square < 10000; square += 197) {
            const int column = 99 - square % 100;
            const int row = square / 100;
            const int dx = column - square_x;
            const int dy = row - square_y;
            const int screen_x = square_offx + 48 * dx + 32 * dy;
            const int screen_y = square_offy - 12 * dx + 24 * dy;
            std::cout << "S " << center << " " << square << " " << screen_x
                      << " " << screen_y << " " << screen_y - 96 << "\n";
        }
    }
    return 0;
}
