#include "../ServerFrameLimit.h"
#include <cassert>
int main() {
    assert(ParseServerFrameLimit(nullptr) == 30);
    assert(ParseServerFrameLimit(L"") == 30);
    assert(ParseServerFrameLimit(L"0") == 0);
    assert(ParseServerFrameLimit(L"60") == 60);
    assert(ParseServerFrameLimit(L"-1") == 30);
    assert(ParseServerFrameLimit(L"300") == 30);
    assert(ParseServerFrameLimit(L"30junk") == 30);
    assert(ParseServerFrameLimit(L"99999999999999999") == 30);
}
