#include "../CityIdlePolicy.h"
#include <cassert>
int main() {
    CityIdlePolicy loading;
    assert(!loading.Advance(179, false));
    assert(!loading.Advance(1, true));
    assert(loading.Advance(0, false));
    CityIdlePolicy abandoned;
    assert(!abandoned.Advance(179, false));
    assert(abandoned.Advance(1, false));
    CityIdlePolicy occupied;
    assert(!occupied.Advance(10000, true));
    assert(!occupied.Advance(10000, true));
    assert(occupied.Advance(0, false));
}
