#include "../TrialsRotation.h"
#include <cassert>
#include <set>

using namespace TrialsRotation;

int main() {
    assert(Sha256Hex("") == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert(Sha256Hex("abc") == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert(Sha256Hex(std::string(100, 'a')) == "2816597888e4a0d3a36b82b83316ab32680eb8f00f8cd3b904d681246d285a0e");

    assert(WeekAt(EpochUnix) == 0);
    assert(WeekAt(EpochUnix - 1) == -1);
    assert(WeekAt(1791399549) == 376); // 2026-10-07
    assert(UnixToTicks(0) == 621355968000000000LL);

    // Pinned from the deploy server's GetTrialsData; all three components must agree.
    const std::pair<int64_t, const char*> Expected[] = {
        {0, "045"}, {1, "075"}, {2, "029"}, {68, "064"}, {377, "039"}, {378, "069"}, {379, "087"}};
    for (auto& [Week, Suffix] : Expected)
        assert(SuffixForWeek(Week) == Suffix);

    std::string Previous;
    for (int64_t Week = -30; Week < 400; ++Week) {
        const std::string Behemoth = BehemothForWeek(Week);
        assert(Behemoth.size() > 6 && Behemoth.compare(Behemoth.size() - 6, 6, "_alpha") == 0);
        assert(Behemoth != Previous);
        Previous = Behemoth;
    }

    std::string Suffix;
    assert(TrialRowSuffix("Arena_MatchmakerHunt_Hard_024", Suffix) && Suffix == "024");
    assert(TrialRowSuffix("Arena_MatchmakerHunt_Elite_001", Suffix) && Suffix == "001");
    assert(!TrialRowSuffix("Arena_MatchmakerHunt_Hard_Test_Cert", Suffix));
    assert(!TrialRowSuffix("CR19_PlayerHunt_Arena_Hard", Suffix));
    assert(!TrialRowSuffix("Arena_MatchmakerHunt_Hard_02", Suffix));

    const int64_t Count = static_cast<int64_t>(Pool().size());
    std::set<std::string> Seen;
    for (int64_t Week = 5 * Count; Week < 6 * Count; ++Week) Seen.insert(BehemothForWeek(Week));
    assert(static_cast<int64_t>(Seen.size()) == Count);
}
