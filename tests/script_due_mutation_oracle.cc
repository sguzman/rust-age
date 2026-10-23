// GPL-2.0-or-later. Independent, invented two-procedure CE doEvents
// scheduler oracle, adapted from pinned src/int/intrpret.cc op_call_condition,
// op_cancel, op_cancelall and doEvents conditional-over-timed priority.
// This is NOT the original compiled engine or game-content acceptance.
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    std::ifstream stream(argv[1]);
    if (!stream) return 2;
    std::string name, mode;
    std::int32_t operand, firstTruth;
    std::uint32_t now;
    while (stream >> name >> mode >> operand >> firstTruth >> now) {
        const bool callWhen = mode == "WHEN" || mode == "WHEN_TIMED" || mode == "WHEN_BAD";
        const bool cancel = mode == "CANCEL" || mode == "CANCEL_BAD";
        const bool cancelAll = mode == "CANCEL_ALL";
        if (!callWhen && !cancel && !cancelAll) return 2;
        if (mode == "WHEN_BAD" || mode == "CANCEL_BAD") {
            std::cout << name << " ERR\n";
            continue;
        }
        unsigned readyMask = 0;
        std::int32_t firstFlags = 0x02;
        std::int32_t secondFlags = callWhen ? (mode == "WHEN_TIMED" ? 0x01 : 0) : 0x02;
        std::int32_t secondConditionOffset = callWhen ? 0 : 28;
        std::int32_t secondEventTime = 0;
        if (callWhen) {
            // CALL_WHEN registers a later condition during the current poll.
            secondFlags |= 0x02;
            secondConditionOffset = 28;
            // Conditional priority is higher than the existing timed flag,
            // even when the event_time (zero) is earlier than 'now'.
            if (operand != 0) {
                readyMask |= 2;
                secondFlags = 0;
            }
        } else {
            // CE CANCEL clears flags, condition and event time. CANCEL_ALL
            // additionally clears the current procedure's own flags.
            if (cancelAll) firstFlags = 0;
            if (firstTruth != 0) {
                readyMask |= 1;
                firstFlags = 0;
            }
            secondFlags = 0;
            secondConditionOffset = 0;
            secondEventTime = 0;
        }
        std::cout << name << " S:" << readyMask
                  << ":" << firstFlags << ":" << secondFlags
                  << ":" << secondEventTime << ":" << secondConditionOffset
                  << ":0\n";
    }
}
