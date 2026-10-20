#pragma once
#include <algorithm>
#include <chrono>
#include <ctime>
#include <iostream>
#include <string>
#include <vector>

#include "DataTables.h"
#include "NativeDiagnostics.h"
#include "SDK.hpp"
#include "TrialsRotation.h"

// The 1.4.4 Trials come from the cooked gen_arena_hunts_schedule table, whose 88 weekly rows ended in
// March 2021. Rewrite those rows in memory to cover last week and the weeks ahead with the servers'
// pick (TrialsRotation.h). Runs in the client and in every game server:
//  - on the server, the scheduler replicates the current Hard/Elite row ids to each player;
//  - on the client, the Trials countdown reads its end time from the row holding the current id.
// The Trials screen itself shows the first of the player hunt's rows that IsHuntUnlocked accepts
// (UPlayerArenaComponent::GetCurrentMatchmakerHuntId), so IsTrialRowUnlocked below lets through only
// the row the server's schedule marks as current.
namespace TrialsSchedule {

inline SDK::UDataTable* Table = nullptr;
inline int32_t TableIndex = -1;
inline int64_t PatchedFirstTicks = 0;
inline std::chrono::steady_clock::time_point NextCheck{};
inline int MissingChecks = 0;

inline int64_t NowUnix() {
    return static_cast<int64_t>(std::time(nullptr));
}

inline SDK::UDataTable* FindTable() {
    return FindDataTable("gen_arena_hunts_schedule");
}

// The weekly rows (Scheduled_Arena_Hunt_001..088) in name order; the daily test rows stay expired.
inline std::vector<SDK::FScheduleData*> WeeklyRows(SDK::UDataTable* DataTable) {
    std::vector<std::pair<std::string, SDK::FScheduleData*>> Rows;
    auto& Map = DataTable->RowMap;

    for (int I = 0; I < Map.NumAllocated(); ++I) {
        if (!Map.IsValidIndex(I))
            continue;
        const std::string Name = Map[I].Key().ToString();
        if (Name.starts_with("Scheduled_Arena_Hunt_") && Map[I].Value())
            Rows.emplace_back(Name, reinterpret_cast<SDK::FScheduleData*>(Map[I].Value()));
    }

    std::sort(Rows.begin(), Rows.end(), [](auto& A, auto& B) { return A.first < B.first; });
    std::vector<SDK::FScheduleData*> Out;
    for (auto& Row : Rows) Out.push_back(Row.second);
    return Out;
}

inline int64_t& Ticks(SDK::FDateTime& Time) {
    return *reinterpret_cast<int64_t*>(&Time);
}

inline bool Patch(SDK::UDataTable* DataTable) {
    auto Rows = WeeklyRows(DataTable);
    if (Rows.empty())
        return false;

    // Row 0 is last week, so the leaderboard's Previous tab resolves too.
    const int64_t FirstWeek = TrialsRotation::WeekAt(NowUnix()) - 1;

    for (size_t K = 0; K < Rows.size(); ++K) {
        const int64_t Week = FirstWeek + static_cast<int64_t>(K);
        const std::string Suffix = TrialsRotation::SuffixForWeek(Week);
        auto* Row = Rows[K];

        Ticks(Row->StartTime) = TrialsRotation::UnixToTicks(TrialsRotation::WeekStartUnix(Week));
        Ticks(Row->EndTime) = TrialsRotation::UnixToTicks(TrialsRotation::WeekStartUnix(Week + 1));
        Row->IsRepeatable = false;

        for (int I = 0; I < Row->ScheduledItems.Num(); ++I) {
            auto& Item = Row->ScheduledItems[I];
            const std::string Current = Item.ID.ToString();
            const wchar_t* Difficulty = Current.contains("_Elite_") ? L"Elite" : Current.contains("_Hard_") ? L"Hard" : nullptr;
            if (!Difficulty)
                continue;

            const std::wstring Id = std::wstring(L"Arena_MatchmakerHunt_") + Difficulty + L"_" + std::wstring(Suffix.begin(), Suffix.end());
            Item.ID = SDK::UKismetStringLibrary::Conv_StringToName(Id.c_str());
        }
    }

    PatchedFirstTicks = Ticks(Rows[0]->StartTime);
    char Line[256] = {};
    const int Size = sprintf_s(Line, "trials_schedule rows=%zu first_week=%lld current=%s (%s)\n", Rows.size(),
        static_cast<long long>(FirstWeek), TrialsRotation::SuffixForWeek(FirstWeek + 1).c_str(), TrialsRotation::BehemothForWeek(FirstWeek + 1).c_str());
    if (Size > 0) {
        std::cout << Line;
        DR_WriteDiagnostic(Line, static_cast<DWORD>(Size));
    }
    return true;
}

// Call from a game-thread tick. Cheap after the first patch: it only re-checks every few seconds
// that the table object and the patched dates are still in place (re-patches if the table reloads).
inline void Tick() {
    const auto Now = std::chrono::steady_clock::now();
    if (Now < NextCheck)
        return;
    NextCheck = Now + std::chrono::seconds(Table ? 30 : 2);

    // Compare by the remembered GObjects slot so a freed table is never dereferenced.
    if (Table && SDK::UObject::GObjects->GetByIndex(TableIndex) == Table) {
        auto Rows = WeeklyRows(Table);
        if (!Rows.empty() && Ticks(Rows[0]->StartTime) == PatchedFirstTicks)
            return;
    }

    Table = FindTable();
    if (Table && !Patch(Table))
        Table = nullptr;
    TableIndex = Table ? Table->Index : -1;

    // Reported once, after about a minute of looking, so a missing table shows up in the log.
    if (!Table && ++MissingChecks == 30)
        DR_WriteDiagnostic("trials_schedule table_missing\n", 30);
}

// The current weekly row as the server's scheduler replicated it to this player, or "" if it has
// none (no player controller yet, or a server without this schedule patch).
inline std::string ReplicatedTrialSuffix(SDK::AArchonPlayerController* PC) {
    if (!PC)
        return {};
    SDK::USchedulerComponent* Scheduler = PC->GetSchedulerComponent();
    if (!Scheduler)
        return {};

    const int64_t NowTicks = TrialsRotation::UnixToTicks(NowUnix());
    std::string Best;
    int64_t BestEnd = 0;
    auto& Items = Scheduler->AvailableScheduledItems;

    for (int I = 0; I < Items.Num(); ++I) {
        std::string Suffix;
        if (!TrialsRotation::TrialRowSuffix(Items[I].ID.ToString(), Suffix) || Items[I].EndTime <= NowTicks)
            continue;
        if (Best.empty() || Items[I].EndTime < BestEnd) {
            Best = Suffix;
            BestEnd = Items[I].EndTime;
        }
    }
    return Best;
}

// IsHuntUnlocked for a weekly Trial row: only this week's row. Returns false in Handled for any other
// hunt id so the caller keeps its own rule.
inline bool IsTrialRowUnlocked(const std::string& HuntId, SDK::AArchonPlayerController* PC, bool& Handled) {
    std::string Suffix;
    Handled = TrialsRotation::TrialRowSuffix(HuntId, Suffix);
    if (!Handled)
        return false;

    std::string Current = ReplicatedTrialSuffix(PC);
    if (Current.empty()) {
        // Same pick from this machine's clock, computed once per week (the screen asks for all 88 rows).
        static int64_t CachedWeek = INT64_MIN;
        static std::string CachedSuffix;
        const int64_t Week = TrialsRotation::WeekAt(NowUnix());
        if (Week != CachedWeek) {
            CachedSuffix = TrialsRotation::SuffixForWeek(Week);
            CachedWeek = Week;
        }
        Current = CachedSuffix;
    }
    return Suffix == Current;
}

}
