#pragma once
#include <windows.h>
#include <cwchar>
#include <cstdio>
#include <cwctype>
#include <cstring>
#include <map>
#include "NoPlatinumPrice.h"
#include "StoreCategoryFix.h"
#include "RamsStoreProtocol.h"
#include "SDK.hpp"
#include "MinHook/MinHook.h"

// Flag-gated Rams main store. Event01 is its wire currency slot;
// normal event-vendor calls keep their original currency.
namespace RamsStorePilot {
inline bool Enabled=false;
// Native store rendering often bypasses ProcessEvent. Restrict fallback mapping
// to a currently visible GeneralStoreScreen, never an NPC/Trials/Prestige store.
inline bool GeneralStoreVisible() {
    static thread_local SDK::UGeneralStoreScreen* Screen=nullptr;
    static thread_local int32_t Index=-1;
    static thread_local ULONGLONG NextScan=0;
    auto* Objects=SDK::UObject::GObjects.operator->();
    if(!Objects) return false;
    const auto Now=GetTickCount64();
    if(Now>=NextScan) {
        NextScan=Now+250; Screen=nullptr; Index=-1;
        for(int32_t i=0;i<Objects->Num();++i) {
            auto* O=Objects->GetByIndex(i);
            if(!O || O->IsDefaultObject() || !O->IsA(SDK::UGeneralStoreScreen::StaticClass())) continue;
            auto* S=static_cast<SDK::UGeneralStoreScreen*>(O);
            if(S->bDestructCalled || S->Visibility==SDK::ESlateVisibility::Hidden || S->Visibility==SDK::ESlateVisibility::Collapsed) continue;
            Screen=S; Index=i; break;
        }
    }
    return Screen && Objects->GetByIndex(Index)==Screen && !Screen->bDestructCalled && Screen->Visibility!=SDK::ESlateVisibility::Hidden && Screen->Visibility!=SDK::ESlateVisibility::Collapsed;
}
inline bool UseRams() { return Enabled && GeneralStoreVisible(); }
using Currency=SDK::EOnlineStorePhoenixCurrency;
using DisplayFn=bool(__fastcall*)(SDK::UArchonGlobalUI*,const Currency*,SDK::FCurrencyDisplayInfo*);
using SoftTexture=SDK::TSoftObjectPtr<SDK::UTexture2D>;
using IconFn=SoftTexture*(__fastcall*)(SDK::UArchonGlobalUI*,SoftTexture*,const Currency*);
inline DisplayFn OriginalDisplay=nullptr;
inline IconFn OriginalIcon=nullptr;
inline SoftTexture RamsIcon{};
inline bool HaveRamsIcon=false;
inline bool __fastcall Display(SDK::UArchonGlobalUI* Self,const Currency* Value,SDK::FCurrencyDisplayInfo* Out) {
    const Currency Rams=Currency::Rams;
    return OriginalDisplay(Self,UseRams() && Value && *Value==Currency::Event01 ? &Rams : Value,Out);
}
inline SoftTexture* __fastcall Icon(SDK::UArchonGlobalUI* Self,SoftTexture* Out,const Currency* Value) {
    const Currency Rams=Currency::Rams;
    const bool Active=UseRams();
    auto* Result=OriginalIcon(Self,Out,Active && Value && *Value==Currency::Event01 ? &Rams : Value);
    if(Active && Value && (*Value==Currency::Event01 || *Value==Currency::Rams) && Result) {RamsIcon=*Result;HaveRamsIcon=true;}
    return Result;
}
inline void PatchGridIcon(SDK::UObject* Object) {
    static thread_local bool Updating=false;
    if(Updating || !Enabled || !Object || !GeneralStoreVisible()) return;
    const auto Class=Object->Class->GetName();
    if(Class!="w_store_item_bpw_C" && Class!="w_purchase_button_C") return;
    if(!HaveRamsIcon) {
        auto* Objects=SDK::UObject::GObjects.operator->();
        for(int32_t i=0;Objects && i<Objects->Num();++i) {
            auto* O=Objects->GetByIndex(i);
            if(!O || O->IsDefaultObject() || !O->IsA(SDK::UArchonGlobalUI::StaticClass())) continue;
            const Currency Rams=Currency::Rams;
            OriginalIcon(static_cast<SDK::UArchonGlobalUI*>(O),&RamsIcon,&Rams);
            HaveRamsIcon=true;break;
        }
        if(!HaveRamsIcon) return;
    }
    SDK::UImage* Image=nullptr;
    if(Class=="w_store_item_bpw_C") {
        auto* Model=static_cast<SDK::UStoreItemWidget*>(Object)->StoreItemViewModel;
        if(!Model || Model->StoreCurrency!=SDK::EArchonStoreCurrency::Event01) return;
        Image=NoPlatinumPrice::Widget<SDK::UImage>(Object,"price_icon");
    } else if(Class=="w_purchase_button_C") {
        if(static_cast<SDK::UPurchaseButton*>(Object)->StoreCurrency!=SDK::EArchonStoreCurrency::Event01) return;
        Image=NoPlatinumPrice::Widget<SDK::UImage>(Object,"platinum_icon");
    }
    if(!Image) return;
    static thread_local std::map<int32_t,ULONGLONG> Updated;
    const auto Now=GetTickCount64();
    if(Updated[Image->Index]+250>Now) return;
    if(Updated.size()>4096) Updated.clear();
    Updated[Image->Index]=Now;
    auto* Fn=Image->Class->GetFunction("Image","SetBrushFromSoftTexture");
    if(!Fn) return;
    struct {SoftTexture Texture;bool MatchSize;uint8_t Pad[7];} Params{RamsIcon,false,{}};
    Updating=true;Image->ProcessEvent(Fn,&Params);Updating=false;
}
struct Scope {
    SDK::UObject* Target=nullptr;
    Scope(SDK::UObject* Object,SDK::UFunction*,void*) {
        if(Enabled && Object && (Object->IsA(SDK::UStoreItemWidget::StaticClass()) || Object->IsA(SDK::UPurchaseButton::StaticClass()))) Target=Object;
    }
    ~Scope() {if(Target) PatchGridIcon(Target);}
};
inline void Install(uintptr_t Base) {
    if(!RamsStoreProtocol::Requested(GetCommandLineW())) return;
    // Guard the four verified 1.4.4 entry points before installing any hooks.
    const uint8_t DisplayBytes[]={0x40,0x57,0x41,0x54,0x41,0x56,0x48,0x81,0xec,0xa0,0x03,0x00};
    const uint8_t IconBytes[]={0x48,0x89,0x5c,0x24,0x18,0x57,0x48,0x83,0xec,0x20,0x8b,0x81};
    const uint8_t TopBytes[]={0x4c,0x8b,0xdc,0x53,0x56,0x41,0x55,0x48,0x83,0xec,0x50,0x48};
    const uint8_t ContentBytes[]={0x40,0x53,0x48,0x81,0xec,0x70,0x01,0x00,0x00,0x48,0x8b,0x05};
    if(std::memcmp(reinterpret_cast<void*>(Base+0x16BCF70),DisplayBytes,sizeof DisplayBytes) ||
       std::memcmp(reinterpret_cast<void*>(Base+0x16BEAC0),IconBytes,sizeof IconBytes) ||
       std::memcmp(reinterpret_cast<void*>(Base+0x198D210),TopBytes,sizeof TopBytes) ||
       std::memcmp(reinterpret_cast<void*>(Base+0x198CB00),ContentBytes,sizeof ContentBytes)) return;
    void* A=reinterpret_cast<void*>(Base+0x16BCF70);
    void* B=reinterpret_cast<void*>(Base+0x16BEAC0);
    if(MH_CreateHook(A,reinterpret_cast<void*>(&Display),reinterpret_cast<void**>(&OriginalDisplay))!=MH_OK) return;
    if(MH_CreateHook(B,reinterpret_cast<void*>(&Icon),reinterpret_cast<void**>(&OriginalIcon))!=MH_OK) {MH_RemoveHook(A);return;}
    if(!StoreCategoryFix::Install(Base,nullptr) || MH_EnableHook(A)!=MH_OK || MH_EnableHook(B)!=MH_OK) {
        MH_DisableHook(A);MH_DisableHook(B);MH_RemoveHook(A);MH_RemoveHook(B);
        StoreCategoryFix::Uninstall(Base);
        return;
    }
    Enabled=true;
}
}
