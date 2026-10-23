// GPL-2.0-or-later: numerical make_path_func behavior adapted from pinned
// Fallout 1 CE src/game/anim.cc and tile.cc. No commercial game assets.
// Standalone C++ oracle to detect Rust behavioral drift.
#include <array>
#include <algorithm>
#include <cstdlib>
#include <iostream>
#include <string>
#include <vector>

struct Node {
    int tile;
    int from;
    int rotation;
    int heuristic;
    int cost;
};

static std::pair<int, int> tile_coord(int tile) {
    int center_x = 199 - 20099 % 200;
    int center_y = 20099 / 200;
    int offset_x = (640 - 32) / 2;
    int offset_y = (380 - 16) / 2;
    if (center_x & 1) {
        --center_x;
        offset_x -= 32;
    }
    const int col = 199 - tile % 200;
    const int row = tile / 200;
    const int delta = col - center_x;
    int x = offset_x + 48 * (delta / 2);
    int y = offset_y + 12 * (delta / -2);
    if (col & 1) {
        if (col <= center_x) {
            x -= 16;
            y += 12;
        } else {
            x += 32;
        }
    }
    x += 16 * (row - center_y);
    y += 12 * (row - center_y);
    return {x, y};
}

static int idist(int a, int b) {
    const auto [x1, y1] = tile_coord(a);
    const auto [x2, y2] = tile_coord(b);
    const int dx = std::abs(x2 - x1);
    const int dy = std::abs(y2 - y1);
    return dx + dy - std::min(dx, dy) / 2;
}

static int neighbor(int tile, int rotation) {
    if (tile < 200 || tile >= 39800 || tile % 200 == 0 || tile % 200 == 199) {
        return tile;
    }
    const int direction[2][6] = {
        {-1, 199, 200, 201, 1, -200},
        {-201, -1, 200, 1, -199, -200},
    };
    return tile + direction[(tile % 200) & 1][rotation];
}

static bool passable(int tile, int mode) {
    if (mode == 0) return true;
    if (mode == 1) return tile != 20098 && tile != 20097 && tile != 19898;
    return false;
}

// CE's seen-on-enqueue priority search and stable tie handling.
static std::vector<int> path(int from, int to, int mode, bool not_in_combat, bool check_destination, bool& found) {
    found = false;
    if (check_destination && !passable(to, mode)) return {};
    std::array<unsigned char, 5000> seen{};
    std::array<Node, 2000> open{};
    std::array<Node, 2000> closed{};
    for (auto& node : open) node.tile = -1;
    seen[from / 8] |= static_cast<unsigned char>(1 << (from & 7));
    open[0] = {from, -1, 0, idist(from, to), 0};
    int open_len = 1;
    int closed_len = 0;
    Node current{};

    while (open_len != 0) {
        int selected = -1;
        int inspected = 0;
        for (int idx = 0; inspected < open_len; ++idx) {
            const auto& node = open[idx];
            if (node.tile == -1) continue;
            ++inspected;
            if (selected == -1 ||
                node.heuristic + node.cost < open[selected].heuristic + open[selected].cost) {
                selected = idx;
            }
        }
        current = open[selected];
        open[selected].tile = -1;
        --open_len;
        if (current.tile == to) {
            found = true;
            break;
        }
        closed[closed_len++] = current;
        if (closed_len == 2000) return {};
        for (int dir = 0; dir < 6; ++dir) {
            const int next = neighbor(current.tile, dir);
            const int bit = 1 << (next & 7);
            if (seen[next / 8] & bit) continue;
            if (next != to && !passable(next, mode)) continue;

            int slot = 0;
            for (; slot < 2000; ++slot) if (open[slot].tile == -1) break;
            ++open_len;
            if (open_len == 2000) return {};
            seen[next / 8] |= static_cast<unsigned char>(bit);
            open[slot] = {next, current.tile, dir, idist(next, to),
                          current.cost + 50 + ((not_in_combat && current.rotation != dir) ? 10 : 0)};
        }
    }

    if (!found) return {};
    std::vector<int> rotations;
    for (int index = 0; index < 800 && current.tile != from; ++index) {
        rotations.push_back(current.rotation);
        int parent = 0;
        while (parent < closed_len && closed[parent].tile != current.from) ++parent;
        if (parent >= closed_len) { found = false; return {}; }
        current = closed[parent];
    }
    std::reverse(rotations.begin(), rotations.end());
    return rotations;
}

int main() {
    struct Test { int from, to, mode; bool not_combat, check_dest; };
    const std::array<Test, 10> cases{{
        {20099, 20098, 0, false, false},
        {20099, 20099, 0, true, false},
        {20099, 20297, 1, true, false},
        {20099, 30000, 0, true, false},
        {20099, 20297, 2, true, false},
        {20099, 20098, 2, true, true},
        {20099, 20098, 2, true, false},
        {20099, 20100, 0, false, false},
        {20098, 19999, 1, true, false},
        {30000, 30099, 0, false, false},
    }};
    for (const auto& t : cases) {
        bool found = false;
        const auto result = path(t.from, t.to, t.mode, t.not_combat, t.check_dest, found);
        std::cout << "P " << t.from << " " << t.to << " " << t.mode << " "
                  << t.not_combat << " " << t.check_dest << " ";
        if (!found) {
            std::cout << "none";
        } else if (result.empty()) {
            std::cout << "-";
        } else {
            for (size_t i = 0; i < result.size(); ++i) {
                if (i) std::cout << ",";
                std::cout << result[i];
            }
        }
        std::cout << "\n";
    }
}
