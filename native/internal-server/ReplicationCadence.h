#pragma once
#include <algorithm>
#include <cmath>
#include <unordered_map>

// Respect low authored update rates without delaying movement, initial actor
// delivery, or urgent teardown. A conservative 10 Hz floor limits state delay.
class ReplicationCadence {
    struct Entry { int Index; double Next; double Seen; };
    std::unordered_map<const void*, Entry> Entries;
    double NextPrune = 0;
public:
    bool Due(const void* Actor, int Index, double Now, float Frequency, bool Urgent) {
        if (Urgent || !std::isfinite(Frequency) || Frequency <= 0 || Frequency >= 30) return true;
        const double Interval = 1.0 / (std::max)(10.0f, Frequency);
        auto [It, New] = Entries.try_emplace(Actor, Entry{Index, Now + Interval, Now});
        if (New) return true;
        auto& State = It->second;
        if (State.Index != Index || Now < State.Seen) {
            State = {Index, Now + Interval, Now};return true;
        }
        State.Seen = Now;
        if (Now + 1e-9 < State.Next) return false;
        State.Next = (std::max)(State.Next + Interval, Now + Interval * 0.5);
        return true;
    }
    void Prune(double Now) {
        if (Now < NextPrune) return;
        NextPrune = Now + 1;
        for (auto It=Entries.begin(); It!=Entries.end();) {
            if (Now - It->second.Seen > 1) It=Entries.erase(It);else ++It;
        }
    }
    void Clear() { Entries.clear();NextPrune=0; }
};
