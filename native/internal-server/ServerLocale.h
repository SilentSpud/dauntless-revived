#pragma once
#include <clocale>

// The hook replaces the deployer's command line, so culture must be set here.
inline constexpr const wchar_t* ServerCommandLine =
    L"Dauntless-Win64-Shipping.exe -server -unattended -nullrhi -nosound -EpicPortal -RepDriverDisable -CULTURE=en-US";

inline void ConfigureServerLocale() {
    _configthreadlocale(_ENABLE_PER_THREAD_LOCALE);
    setlocale(LC_ALL, "C");
    _configthreadlocale(_DISABLE_PER_THREAD_LOCALE);
    setlocale(LC_ALL, "C");
}
