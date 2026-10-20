#pragma once
#include "SDK.hpp"

// The backend owns the award; this client only asks the owned
// inventory actor to reload it and reports an observed balance change.
namespace CreatorRewardRefresh {
inline SDK::AArchonInventory* LocalInventory() {
    for(int32_t I = 0; I < SDK::UObject::GObjects->Num(); ++I){
        auto* Object = SDK::UObject::GObjects->GetByIndex(I);
        if(!Object || Object->IsDefaultObject() || !Object->IsA(SDK::AArchonPlayerController::StaticClass())) continue;
        auto* Controller = static_cast<SDK::AArchonPlayerController*>(Object);
        if(Controller->Player && Controller->Player->IsA(SDK::ULocalPlayer::StaticClass())) return Controller->GetInventory();
    }
    return nullptr;
}
inline void Notify(const wchar_t* Message) {
    for(int32_t I = 0; I < SDK::UObject::GObjects->Num(); ++I){
        auto* Object = SDK::UObject::GObjects->GetByIndex(I);
        if(!Object || Object->IsDefaultObject() || !Object->IsA(SDK::UArchonGlobalUI::StaticClass())) continue;
        static_cast<SDK::UArchonGlobalUI*>(Object)->DisplayToastMessage(
            SDK::UKismetTextLibrary::Conv_StringToText(SDK::FString(Message)), SDK::EToastMessageType::Information, nullptr);
        return;
    }
}
inline void OnProcessEvent(SDK::UObject* Object, SDK::UFunction* Function) {
    static bool Updating = false;
    if(Updating) return;
    struct Guard { bool& Value; Guard(bool& V): Value(V) { Value = true; } ~Guard() { Value = false; } } Guard(Updating);
    static int32_t ScreenIndex = -1, InventoryIndex = -1, Before = -1, Attempts = 0;
    static SDK::AArchonInventory* Inventory = nullptr;
    static bool SupportedVoid = false;
    static ULONGLONG Until = 0, NextPoll = 0, NextRefresh = 0;
    const auto Now = GetTickCount64();
    if(Object && !Object->IsDefaultObject() && Object->IsA(SDK::USupportCreatorScreen::StaticClass())){
        const auto Name = Function ? Function->GetName() : "";
        auto* Screen = static_cast<SDK::USupportCreatorScreen*>(Object);
        if(Screen->CurrentCreatorName.TextData) SupportedVoid = Screen->CurrentCreatorName.ToString() == "Void";
        if(ScreenIndex != Object->Index || Name == "OnNameTextChanged" || Name == "OnCloseScreen"){
            ScreenIndex = Object->Index;
            Until = Now + 15000;
            NextRefresh = Now + 750;
            Attempts = 0;
            if(!Inventory){
                Inventory = LocalInventory();
                if(Inventory){ InventoryIndex = Inventory->Index; Before = Inventory->GetInventoryCurrencyAmount(SDK::FString(L"CURRENCY_NOTES")); }
            }
        }
    }
    if(Until && Now > Until){ Until = 0; Inventory = nullptr; Before = -1; }
    if(!Until || Now < NextPoll) return;
    NextPoll = Now + 250;
    if(!Inventory || SDK::UObject::GObjects->GetByIndex(InventoryIndex) != Inventory){
        Until = 0; Inventory = nullptr; Before = -1; return;
    }
    const auto Balance = Inventory->GetInventoryCurrencyAmount(SDK::FString(L"CURRENCY_NOTES"));
    if(Before >= 0 && Balance != Before){
        Until = 0;
        if(SupportedVoid && static_cast<int64_t>(Balance) - Before == 10000) Notify(L"Void redeemed: +10,000 Rams.");
        Inventory = nullptr; Before = -1;
        return;
    }
    if(Attempts < 3 && Now >= NextRefresh){
        ++Attempts;
        NextRefresh = Now + 3000;
        Inventory->ServerRefreshInventory();
    }
}
}
