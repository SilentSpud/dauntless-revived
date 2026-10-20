#include "../ServerLocale.h"
#include <cassert>
#include <cstdlib>
#include <cwchar>
#include <future>
#include <iostream>

static bool ReadsDecimalPoint() {
    char* end = nullptr;
    const auto value = std::strtod("105.6", &end);
    return value == 105.6 && end && *end == '\0';
}

int main() {
    assert(std::wcsstr(ServerCommandLine, L"-CULTURE=en-US"));
    for (const auto locale : {"French_France.1252", "German_Germany.1252", "Italian_Italy.1252"}) {
        _configthreadlocale(_DISABLE_PER_THREAD_LOCALE);
        assert(setlocale(LC_ALL, locale));
        assert(!ReadsDecimalPoint()); // Reproduces the old parsing failure.
        _configthreadlocale(_ENABLE_PER_THREAD_LOCALE);
        assert(setlocale(LC_ALL, locale));
        ConfigureServerLocale();
        assert(ReadsDecimalPoint());
        assert(std::async(std::launch::async, ReadsDecimalPoint).get());
    }
    std::cout << "server locale: French/German/Italian decimal parsing repaired on current and new threads\n";
}
