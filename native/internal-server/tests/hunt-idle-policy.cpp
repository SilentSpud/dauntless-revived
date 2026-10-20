#include "../HuntIdlePolicy.h"
#include <cassert>
int main() {
    HuntIdlePolicy p;
    assert(!p.Advance(50,false));
    assert(!p.Advance(69,false));
    assert(!p.Advance(1,true));
    assert(!p.Advance(119,false));
    assert(!p.Advance(1,true));
    assert(!p.Advance(119,false));
    assert(p.Advance(1,false));
    HuntIdlePolicy abandoned;
    assert(!abandoned.Advance(119,false));
    assert(abandoned.Advance(1,false));
    HuntIdlePolicy invalidTime;
    assert(!invalidTime.Advance(-100,false));
    assert(!invalidTime.Advance(119,false));
    assert(invalidTime.Advance(1,false));
    HuntIdlePolicy occupied;
    assert(!occupied.Advance(600,true));
    assert(!occupied.Advance(119,false));
    assert(!occupied.Advance(600,true));
    assert(!occupied.Advance(119,false));
    assert(occupied.Advance(1,false));
}
