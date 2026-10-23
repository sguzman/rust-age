// GPL-2.0-or-later, independent synthetic CE script_q_* and queue_add
// differential oracle. Pinned scripts.cc script_q_add/save/load/process,
// queue.cc queue_add/remove and scripts.cc exec_script_proc. No game data.
#include <algorithm>
#include <cstdint>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

struct Event {
    int time, owner, sid, fixed;
};
static std::string payload(int32_t sid, int32_t param) {
    std::ostringstream out;
    for (uint32_t number : {static_cast<uint32_t>(sid), static_cast<uint32_t>(param)}) {
        for (int shift = 24; shift >= 0; shift -= 8) {
            out << std::hex << std::setfill('0') << std::setw(2)
                << ((number >> shift) & 0xff);
        }
    }
    return out.str();
}
static std::string probe(const std::string& label, int now, int delay, int sid,
                         int fixed, int owner, int due) {
    std::vector<Event> events;
    auto enqueue = [&](int eventSid, int eventFixed, int eventOwner) {
        // Original scr_ptr rejects SIDs with no active script record.
        if (eventSid != 500 && eventSid != 600) return;
        const Event ev{now + delay, eventOwner, eventSid, eventFixed};
        const auto at = std::find_if(events.begin(), events.end(),
            [&](const Event& existing) { return existing.time > ev.time; });
        events.insert(at, ev); // queue_add inserts after equal-time events.
    };
    enqueue(sid, fixed, owner);
    if (label == "tie") enqueue(sid, fixed + 1, owner);
    if (label == "remove") {
        enqueue(sid, fixed + 1, owner + 1);
        events.erase(std::remove_if(events.begin(), events.end(),
            [&](const Event& ev) { return ev.owner == owner; }), events.end());
    }
    std::vector<std::string> popped;
    while (!events.empty() && events.front().time <= due) {
        const auto ev = events.front();
        events.erase(events.begin());
        std::string action = ev.sid == 500 ? "22:1" : "none";
        popped.push_back(std::to_string(ev.sid) + ":" + action + ":"
            + std::to_string(ev.fixed) + "@" + std::to_string(ev.time)
            + ":" + payload(ev.sid, ev.fixed));
    }
    if (popped.empty()) {
        return events.empty() ? "NONE:0" : "WAIT@" + std::to_string(events.front().time)
            + ":" + std::to_string(events.size());
    }
    std::string output;
    for (const auto& value : popped) {
        if (!output.empty()) output += ",";
        output += value;
    }
    return output + ":" + std::to_string(events.size());
}
int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream in(argv[1]);
    if (!in) return 2;
    std::string label;
    int now, delay, sid, fixed, owner, due;
    while (in >> label >> now >> delay >> sid >> fixed >> owner >> due) {
        std::cout << label << " "
            << probe(label, now, delay, sid, fixed, owner, due) << "\n";
    }
}
