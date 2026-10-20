#pragma once
#include <string_view>

enum class ServerConsoleMode { Legacy, Hidden, Visible };

inline ServerConsoleMode ServerConsolePolicy(std::string_view value) {
    if (value == "0") return ServerConsoleMode::Hidden;
    if (value == "1") return ServerConsoleMode::Visible;
    return ServerConsoleMode::Legacy;
}

#ifdef _WIN32
#include <Windows.h>
#include <cstdio>

inline void InitServerConsole() {
    char setting[2] = {};
    const DWORD length = GetEnvironmentVariableA("DR_SHOW_CONSOLE", setting, sizeof(setting));
    const auto mode = ServerConsolePolicy(length == 1 ? std::string_view(setting, 1) : std::string_view());

    if (mode == ServerConsoleMode::Hidden) {
        // Only explicit suppression changes allocation. Unset deployments retain
        // their historical AllocConsole/CONOUT$/CONIN$ path without a hide thread.
        struct ConsoleOptions { DWORD mode; BOOL useShowWindow; WORD showWindow; };
        using Allocate = HRESULT(WINAPI*)(ConsoleOptions*, DWORD*);
        const auto allocate = reinterpret_cast<Allocate>(GetProcAddress(GetModuleHandleW(L"kernel32.dll"), "AllocConsoleWithOptions"));
        if (allocate) {
            ConsoleOptions options = { 2u, TRUE, SW_HIDE };
            if (FAILED(allocate(&options, nullptr))) AllocConsole();
        } else {
            AllocConsole();
        }
        if (HWND window = GetConsoleWindow()) ShowWindow(window, SW_HIDE);
        // Legacy conhost can show its window after allocation or screen changes.
        HANDLE watcher = CreateThread(nullptr, 0, [](LPVOID) -> DWORD {
            for (;;) {
                if (HWND window = GetConsoleWindow(); window && IsWindowVisible(window)) ShowWindow(window, SW_HIDE);
                Sleep(50);
            }
        }, nullptr, 0, nullptr);
        if (watcher) CloseHandle(watcher);
    } else {
        AllocConsole();
        if (mode == ServerConsoleMode::Visible) {
            if (HWND window = GetConsoleWindow()) ShowWindow(window, SW_SHOW);
        }
    }
    FILE* stream;
    freopen_s(&stream, "CONOUT$", "w", stdout);
    freopen_s(&stream, "CONIN$", "r", stdin);
}
#endif
