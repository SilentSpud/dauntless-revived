#pragma once
#include <chrono>

#include "SDK.hpp"

// The 1.4.4 "Press Start" screen (Login/PressStartScreen_bps) only shows Phoenix's old welcome text
// with an OK button. Press it for the player: once the screen is built, run the same
// Internal_OnDismissRequested the OK button runs (it calls HandleStartPressed and plays the outro).
namespace AutoPressStart {

inline SDK::UObject* Screen = nullptr;
inline int32_t ScreenIndex = -1;
inline std::chrono::steady_clock::time_point DismissAt{};

// Call after every client ProcessEvent.
inline void OnProcessEvent(SDK::UObject* Object, SDK::UFunction* Function) {
    static SDK::UFunction* Construct = nullptr;
    if (Function && (Function == Construct || (!Construct && Function->GetFullName().contains("PressStartScreen_bps_C.Construct")))) {
        Construct = Function;
        Screen = Object;
        ScreenIndex = Object ? Object->Index : -1;
        // A moment after it appears, so the login flow behind it is ready (as for a player clicking).
        DismissAt = std::chrono::steady_clock::now() + std::chrono::milliseconds(750);
        return;
    }

    if (!Screen || std::chrono::steady_clock::now() < DismissAt)
        return;

    SDK::UObject* Target = Screen;
    Screen = nullptr;
    // The slot must still hold the same screen (it may have been closed in the meantime).
    if (SDK::UObject::GObjects->GetByIndex(ScreenIndex) != Target)
        return;

    SDK::UFunction* Dismiss = Target->Class->GetFunction("PressStartScreen_bps_C", "Internal_OnDismissRequested");
    if (!Dismiss)
        return;

    struct { int32_t UserIndex = 0; } Params;
    Target->ProcessEvent(Dismiss, &Params);
}

}
