#pragma once
#include <cwchar>

// Dedicated worlds use real elapsed time, capped at 30 frames/sec by default.
// 0 restores the game's original policy; invalid values keep the safe default.
inline int ParseServerFrameLimit(const wchar_t* Text) {
    if (!Text || !*Text) return 30;
    wchar_t* End = nullptr;
    const long Value = std::wcstol(Text, &End, 10);
    if (End == Text || *End || (Value != 0 && (Value < 10 || Value > 120))) return 30;
    return static_cast<int>(Value);
}
