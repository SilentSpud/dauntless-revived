#pragma once
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <iostream>

#include "DataTables.h"
#include "NativeDiagnostics.h"
#include "SDK.hpp"

// Middleman cell fusion time. The cooked Gameplay/cell_exchange_table gives each fusion recipe its
// TimeRequired (1.4.4: Uncommon->Rare 22 h, Rare->Epic 46 h, Epic->Epic 70 h). Set every recipe to
// FusionHours in memory, in the client (what the Middleman screen shows) and the game servers (what a
// started fusion's end time is built from).
namespace CellFusionTime {

constexpr int64_t FusionHours = 1;
constexpr int64_t TicksPerHour = 36000000000LL; // FTimespan ticks are 100 ns

inline SDK::UDataTable* Table = nullptr;
inline int32_t TableIndex = -1;
inline std::chrono::steady_clock::time_point NextCheck{};

inline int64_t& Ticks(SDK::FTimespan& Time) {
    return *reinterpret_cast<int64_t*>(&Time);
}

// True when every recipe already has the wanted time.
inline bool Patch(SDK::UDataTable* DataTable) {
    auto& Map = DataTable->RowMap;
    int Rows = 0, Changed = 0;

    for (int I = 0; I < Map.NumAllocated(); ++I) {
        if (!Map.IsValidIndex(I) || !Map[I].Value())
            continue;
        auto* Row = reinterpret_cast<SDK::FCellExchangeTableData*>(Map[I].Value());
        ++Rows;
        if (Ticks(Row->TimeRequired) != FusionHours * TicksPerHour) {
            Ticks(Row->TimeRequired) = FusionHours * TicksPerHour;
            ++Changed;
        }
    }

    if (Changed > 0) {
        char Line[96] = {};
        const int Size = sprintf_s(Line, "cell_fusion_time recipes=%d changed=%d hours=%lld\n", Rows, Changed, static_cast<long long>(FusionHours));
        if (Size > 0) {
            std::cout << Line;
            DR_WriteDiagnostic(Line, static_cast<DWORD>(Size));
        }
    }
    return Rows > 0;
}

// Call from a game-thread tick; looks for the table every 2 s until found, then re-checks every 30 s.
inline void Tick() {
    const auto Now = std::chrono::steady_clock::now();
    if (Now < NextCheck)
        return;
    NextCheck = Now + std::chrono::seconds(Table ? 30 : 2);

    // Compare by the remembered GObjects slot so a freed table is never dereferenced.
    if (!(Table && SDK::UObject::GObjects->GetByIndex(TableIndex) == Table)) {
        Table = FindDataTable("cell_exchange_table");
        TableIndex = Table ? Table->Index : -1;
    }
    if (Table && !Patch(Table))
        Table = nullptr;
}

}
