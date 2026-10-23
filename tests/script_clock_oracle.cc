// GPL-2.0-or-later. Adapted CE source-clock C++ oracle.
// Pinned src/int/support/intextra.cc op_game_time,
// op_game_time_in_seconds, op_game_ticks, op_get_month/day and
// game/scripts.cc game_time_date. Invented snapshots only.
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

// The CE date function starts in December 2161 and uses 365-day years.
static void ce_date(int ticks, int& month, int& day, int& year) {
    const int daysInMonth[] = {31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31};
    const int elapsed = ticks / (24 * 60 * 60 * 10) + (5 - 1);
    year = elapsed / 365 + 2161;
    month = 11;
    day = elapsed % 365;
    for (;;) {
        if (day < daysInMonth[month]) break;
        day -= daysInMonth[month];
        month++;
        if (month == 12) { year++; month = 0; }
    }
    month++;
    day++;
}

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream input(argv[1]);
    if (!input) return 2;
    std::string label;
    char kind;
    int time, seconds;
    while (input >> label >> kind >> time >> seconds) {
        std::vector<int> stack;
        int steps, pc;
        switch (kind) {
        case 't':
            stack.push_back(time);
            steps = 2; pc = 4;
            break;
        case 's':
            stack.push_back(time / 10);
            steps = 2; pc = 4;
            break;
        case 'h':
            stack.push_back(100 * ((time / 600) / 60 % 24) + (time / 600) % 60);
            steps = 2; pc = 4;
            break;
        case 'v':
            // scripts.cc lastVisitTime is zero until the MAP has been visited.
            // op_days_since_visited divides signed tick delta by a CE day.
            stack.push_back(seconds == 0 ? -1 : (time - seconds) / 864000);
            steps = 2; pc = 4;
            break;
        case 'm':
        case 'd': {
            int month, day, year;
            ce_date(time, month, day, year);
            stack.push_back(kind == 'm' ? month : day);
            steps = 2; pc = 4;
            break;
        }
        case 'k':
            stack.push_back(seconds < 0 ? 0 : seconds * 10);
            steps = 3; pc = 10;
            break;
        case 'c':
            stack.push_back(time / 10);
            stack.push_back(seconds < 0 ? 0 : seconds * 10);
            {
                int right = stack.back(); stack.pop_back();
                int left = stack.back(); stack.pop_back();
                stack.push_back(left + right);
            }
            steps = 5; pc = 14;
            break;
        default: return 2;
        }
        std::cout << label << " S:" << stack.back()
                  << " N:" << steps << " PC:" << pc << "\n";
    }
}
