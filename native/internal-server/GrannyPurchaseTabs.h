#pragma once
#include "SDK.hpp"
#include <chrono>

// Enable native vendor purchasing before tab creation; native quest/price checks remain.
namespace GrannyPurchaseTabs {
inline void* OriginalFeatureCheck = nullptr;
inline bool FeatureCheck(const SDK::UObject* Context, SDK::UClass* Feature) {
    if (Feature && Feature == SDK::UPurchasesAtVendorFeature::StaticClass()) return true;
    return reinterpret_cast<bool(*)(const SDK::UObject*, SDK::UClass*)>(OriginalFeatureCheck)(Context, Feature);
}
inline void Install(uintptr_t Base) {
    auto* Target = reinterpret_cast<void*>(Base + 0x14B0F70);
    if (MH_CreateHook(Target, FeatureCheck, &OriginalFeatureCheck) == MH_OK)
        MH_EnableHook(Target);
}
inline void Observe(SDK::UObject* Object) {
    static SDK::UCraftingScreen* Screen = nullptr;
    static int32_t Index = -1;
    static std::chrono::steady_clock::time_point NextCheck{};
    if (Object && !Object->IsDefaultObject() && Object->IsA(SDK::UCraftingScreen::StaticClass())) {
        Screen = static_cast<SDK::UCraftingScreen*>(Object);
        Index = Object->Index;
    }
    const auto Now = std::chrono::steady_clock::now();
    if (!Screen || Now < NextCheck) return;
    NextCheck = Now + std::chrono::milliseconds(200);
    if (SDK::UObject::GObjects->GetByIndex(Index) != Screen) { Screen = nullptr; return; }
    if (!Screen->CraftingViewModel || !Screen->CraftingListWidget
        || Screen->CraftingViewModel->EquipmentVendorType != SDK::EEquipmentVendorType::Potions
        || !Screen->ChargeButtonTabs || Screen->ChargeButtonTabs->CachedVisibleTabsCount != 2
        || !Screen->PurchaseTabButton || !Screen->PurchaseTabButton->DataModel
        || !Screen->PurchaseTabButton->SelectedImage) return;
    auto* Container = Screen->ChargeButtonTabsContainer;
    if (!Container || Container->Visibility == SDK::ESlateVisibility::SelfHitTestInvisible) return;
    auto* Function = Container->Class->GetFunction("Widget", "SetVisibility");
    if (!Function) return;
    struct { SDK::ESlateVisibility Value; } Params{SDK::ESlateVisibility::SelfHitTestInvisible};
    Container->ProcessEvent(Function, &Params);
}
}
