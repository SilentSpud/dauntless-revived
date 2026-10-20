#pragma once
#include <algorithm>
#include <array>
#include <cstdint>
#include <cstring>
#include <string>
#include <utility>
#include <vector>

// The weekly Trials pick, identical to UndauntedDeployServer (controllers/gameservers.ts), which
// launches the hunt, and UndauntedMetagame (controllers/trials.ts), which scores it. The game's
// own Trials schedule expired in 2021, so the DLL feeds these picks into it (TrialsSchedule.h).
namespace TrialsRotation {

// Thursday 2019-07-18 18:00 UTC, the deploy server's TRIAL_ROTATION_START.
constexpr int64_t EpochUnix = 1563472800;
constexpr int64_t WeekSeconds = 7 * 24 * 60 * 60;

// vendor/trials_pool.json: elemental variants that also have a Heroic hunt of that variant, each
// with the cooked Trial rows that use it at both difficulties. Keep in sync (a deploy server test
// compares the two).
inline const std::vector<std::pair<std::string, std::vector<std::string>>>& Pool() {
    static const std::vector<std::pair<std::string, std::vector<std::string>>> Rows = {
        {"bullseye_alpha", {"003", "016", "029", "042", "062", "087"}},
        {"electroquill_alpha", {"001", "014", "027", "040"}},
        {"host_alpha", {"045", "055", "071"}},
        {"lerawr_alpha", {"004", "017", "030", "047", "064", "088"}},
        {"mcbeaver_alpha", {"013", "026", "039", "060", "077"}},
        {"mcrollin_alpha", {"012", "025", "038", "075"}},
        {"moonface_alpha", {"010", "023", "036", "056", "073"}},
        {"scissors_alpha", {"006", "019", "032", "050", "067", "081"}},
        {"shockmane_alpha", {"044", "052", "069", "079"}},
        {"snowflake_alpha", {"005", "018", "031", "048", "065", "080"}},
        {"sq_rock_alpha", {"011", "024", "037", "057", "074"}},
        {"sq_sally_alpha", {"009", "022", "035", "054", "072"}},
    };
    return Rows;
}

// SHA-256 as lowercase hex (FIPS 180-4), the seed hash the servers use.
inline std::string Sha256Hex(const std::string& Input) {
    static constexpr std::array<uint32_t, 64> K = {
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2};
    std::array<uint32_t, 8> H = {0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19};
    auto Rotr = [](uint32_t X, int N) { return (X >> N) | (X << (32 - N)); };

    std::vector<uint8_t> Message(Input.begin(), Input.end());
    const uint64_t BitLength = static_cast<uint64_t>(Message.size()) * 8;
    Message.push_back(0x80);
    while (Message.size() % 64 != 56) Message.push_back(0);
    for (int Shift = 56; Shift >= 0; Shift -= 8) Message.push_back(static_cast<uint8_t>(BitLength >> Shift));

    for (size_t Block = 0; Block < Message.size(); Block += 64) {
        std::array<uint32_t, 64> W{};
        for (int I = 0; I < 16; ++I) {
            const uint8_t* P = &Message[Block + I * 4];
            W[I] = (uint32_t(P[0]) << 24) | (uint32_t(P[1]) << 16) | (uint32_t(P[2]) << 8) | uint32_t(P[3]);
        }
        for (int I = 16; I < 64; ++I) {
            const uint32_t S0 = Rotr(W[I - 15], 7) ^ Rotr(W[I - 15], 18) ^ (W[I - 15] >> 3);
            const uint32_t S1 = Rotr(W[I - 2], 17) ^ Rotr(W[I - 2], 19) ^ (W[I - 2] >> 10);
            W[I] = W[I - 16] + S0 + W[I - 7] + S1;
        }
        uint32_t A = H[0], B = H[1], C = H[2], D = H[3], E = H[4], F = H[5], G = H[6], Hh = H[7];
        for (int I = 0; I < 64; ++I) {
            const uint32_t T1 = Hh + (Rotr(E, 6) ^ Rotr(E, 11) ^ Rotr(E, 25)) + ((E & F) ^ (~E & G)) + K[I] + W[I];
            const uint32_t T2 = (Rotr(A, 2) ^ Rotr(A, 13) ^ Rotr(A, 22)) + ((A & B) ^ (A & C) ^ (B & C));
            Hh = G; G = F; F = E; E = D + T1; D = C; C = B; B = A; A = T1 + T2;
        }
        H[0] += A; H[1] += B; H[2] += C; H[3] += D; H[4] += E; H[5] += F; H[6] += G; H[7] += Hh;
    }

    static constexpr char Hex[] = "0123456789abcdef";
    std::string Out;
    for (uint32_t Word : H)
        for (int Shift = 28; Shift >= 0; Shift -= 4) Out.push_back(Hex[(Word >> Shift) & 0xF]);
    return Out;
}

inline int64_t FloorDiv(int64_t Value, int64_t Divisor) {
    return Value / Divisor - ((Value % Divisor != 0) && ((Value < 0) != (Divisor < 0)));
}

inline int64_t WeekAt(int64_t UnixSeconds) {
    return FloorDiv(UnixSeconds - EpochUnix, WeekSeconds);
}

inline int64_t WeekStartUnix(int64_t Week) {
    return EpochUnix + Week * WeekSeconds;
}

// Strings sorted by the hash of Prefix + string.
inline std::vector<std::string> SortedByScore(std::vector<std::string> Items, const std::string& Prefix) {
    std::vector<std::pair<std::string, std::string>> Scored;
    for (auto& Item : Items) Scored.emplace_back(Sha256Hex(Prefix + Item), Item);
    std::sort(Scored.begin(), Scored.end());
    std::vector<std::string> Out;
    for (auto& Entry : Scored) Out.push_back(Entry.second);
    return Out;
}

inline std::vector<std::string> CycleOrder(int64_t Cycle) {
    std::vector<std::string> Names;
    for (auto& Entry : Pool()) Names.push_back(Entry.first);
    std::sort(Names.begin(), Names.end());
    return SortedByScore(Names, "trials-weekly-v1:" + std::to_string(Cycle) + ":");
}

// Every behemoth once per cycle of N weeks, never the same one two weeks running.
inline std::string BehemothForWeek(int64_t Week) {
    const int64_t Count = static_cast<int64_t>(Pool().size());
    const int64_t Cycle = FloorDiv(Week, Count);
    auto Order = CycleOrder(Cycle);

    if (Count > 1 && Order[0] == CycleOrder(Cycle - 1)[Count - 1])
        std::swap(Order[0], Order[1]);

    return Order[static_cast<size_t>(Week - Cycle * Count)];
}

// The row suffix ("024") shared by Arena_MatchmakerHunt_Hard_ and _Elite_ that week.
inline std::string SuffixForWeek(int64_t Week) {
    const std::string Behemoth = BehemothForWeek(Week);
    for (auto& Entry : Pool())
        if (Entry.first == Behemoth)
            return SortedByScore(Entry.second, "trials-weekly-v1:row:" + std::to_string(Week) + ":")[0];
    return {};
}

// A weekly Trial row id: Arena_MatchmakerHunt_Hard_024 or _Elite_024 gives "024".
inline bool TrialRowSuffix(const std::string& Id, std::string& Suffix) {
    for (const char* Prefix : {"Arena_MatchmakerHunt_Hard_", "Arena_MatchmakerHunt_Elite_"}) {
        const size_t Length = std::strlen(Prefix);
        if (Id.size() == Length + 3 && Id.compare(0, Length, Prefix) == 0 &&
            std::all_of(Id.begin() + Length, Id.end(), [](char C) { return C >= '0' && C <= '9'; })) {
            Suffix = Id.substr(Length);
            return true;
        }
    }
    return false;
}

// FDateTime ticks (100 ns since 0001-01-01) for a Unix time.
inline int64_t UnixToTicks(int64_t UnixSeconds) {
    return (UnixSeconds + 62135596800LL) * 10000000LL;
}

}
