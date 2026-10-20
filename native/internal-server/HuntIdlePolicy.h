#pragma once
#include <algorithm>

// Reclaim hunts after two continuous minutes without a pending or open connection.
struct HuntIdlePolicy {
    double EmptySeconds = 0;
    bool Advance(double Seconds, bool HasPlayer) {
        if (HasPlayer) { EmptySeconds = 0; return false; }
        EmptySeconds += std::max(0.0, Seconds);
        return EmptySeconds >= 120.0;
    }
};
