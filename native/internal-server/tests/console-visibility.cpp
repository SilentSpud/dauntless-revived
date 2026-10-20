#include "../ConsoleVisibility.h"
#include <cassert>

int main([[maybe_unused]] int argc, [[maybe_unused]] char** argv) {
    assert(ServerConsolePolicy("") == ServerConsoleMode::Legacy);
    assert(ServerConsolePolicy("0") == ServerConsoleMode::Hidden);
    assert(ServerConsolePolicy("1") == ServerConsoleMode::Visible);
    assert(ServerConsolePolicy("true") == ServerConsoleMode::Legacy);
    assert(ServerConsolePolicy("11") == ServerConsoleMode::Legacy);
#ifdef _WIN32
    // The separate GUI-subsystem test process owns its console; never hide a terminal's.
    if (argc > 1) {
        SetEnvironmentVariableA("DR_SHOW_CONSOLE", "0");
        InitServerConsole();
        Sleep(250);
        assert(!GetConsoleWindow() || !IsWindowVisible(GetConsoleWindow()));
        assert(std::fputs("Hidden diagnostic output is retained\n", stdout) >= 0);
    }
#endif
}
