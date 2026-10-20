#pragma once
#include <algorithm>
struct CityIdlePolicy {
    double WaitingSeconds = 0;
    bool HadConnection = false;
    bool Advance(double Seconds, bool HasConnection) {
        if (HasConnection) { HadConnection = true; return false; }
        WaitingSeconds += std::max(0.0, Seconds);
        return HadConnection || WaitingSeconds >= 180.0;
    }
};
