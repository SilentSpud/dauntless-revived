#pragma once
#include <string>

#include "SDK.hpp"

// Finding a cooked DataTable by its object name, for the in-memory data patches (TrialsSchedule.h,
// CellFusionTime.h). Walks GObjects, so callers look it up rarely and keep the result.
inline SDK::UDataTable* FindDataTable(const std::string& Name) {
    for (int I = 0; I < SDK::UObject::GObjects->Num(); ++I) {
        SDK::UObject* Object = SDK::UObject::GObjects->GetByIndex(I);
        if (!Object || !Object->IsA(SDK::UDataTable::StaticClass()))
            continue;
        if (Object->GetName() == Name && !Object->IsDefaultObject())
            return static_cast<SDK::UDataTable*>(Object);
    }
    return nullptr;
}
