#pragma once
#include "SDK.hpp"
#include "MinHook/MinHook.h"
#include <string>

// 1.4.4 pagination indexes visible labels while store content indexes the
// complete category array. Resolve by the selected localized label and keep
// the pagination's own index unchanged outside the content update.
namespace StoreCategoryFix {
using UpdateFn=void(__fastcall*)(SDK::UGeneralStoreScreen*);
inline UpdateFn OriginalTop=nullptr;
inline UpdateFn OriginalContent=nullptr;
inline void(*Log)(const char*,...)=nullptr;
inline thread_local unsigned TopDepth=0;
inline std::string SelectedLabel(SDK::UArchonPaginationWidget* Menu) {
    if(!Menu || Menu->CurrentPageIndex<0 || Menu->CurrentPageIndex>=Menu->NumPages) return {};
    auto* Item=Menu->GetPageItemWidget(Menu->CurrentPageIndex);
    return Item ? Item->PageText.ToString() : std::string{};
}
inline int CategoryIndex(SDK::UGeneralStoreScreen* Screen) {
    const auto Label=SelectedLabel(Screen->TopMenuWidget);
    if(Label.empty() || !Screen->StoreViewModel) return -1;
    const auto& Categories=Screen->StoreViewModel->StoreCategoryViewModels;
    int Match=-1;
    for(int i=0;i<Categories.Num();++i) if(Categories[i].CategoryName.ToString()==Label) {
        if(Match>=0) return -1; // Ambiguous localized names: leave original behavior.
        Match=i;
    }
    return Match;
}
inline void __fastcall Top(SDK::UGeneralStoreScreen* Screen) {
    if(!Screen || !Screen->TopMenuWidget || !Screen->StoreViewModel || TopDepth) {OriginalTop(Screen);return;}
    auto* Menu=Screen->TopMenuWidget;
    const int Saved=Menu->CurrentPageIndex;
    const int Correct=CategoryIndex(Screen);
    if(Correct>=0) Menu->CurrentPageIndex=Correct;
    if(Log) Log("[rams-store-pilot] category visible=%d model=%d label=%s\n",Saved,Correct,Correct>=0?Screen->StoreViewModel->StoreCategoryViewModels[Correct].CategoryName.ToString().c_str():"unmatched");
    ++TopDepth;
    OriginalTop(Screen);
    --TopDepth;
    Menu->CurrentPageIndex=Saved;
}
inline void __fastcall Content(SDK::UGeneralStoreScreen* Screen) {
    if(!Screen || !Screen->TopMenuWidget || !Screen->StoreViewModel) {OriginalContent(Screen);return;}
    auto* TopMenu=Screen->TopMenuWidget;
    auto* SubMenu=Screen->SubMenuWidget;
    const int SavedTop=TopMenu->CurrentPageIndex;
    const int SavedSub=SubMenu?SubMenu->CurrentPageIndex:0;
    const int CorrectTop=TopDepth?SavedTop:CategoryIndex(Screen);
    const auto& Categories=Screen->StoreViewModel->StoreCategoryViewModels;
    if(CorrectTop>=0 && CorrectTop<Categories.Num()) {
        TopMenu->CurrentPageIndex=CorrectTop;
        if(SubMenu) {
            const auto Label=SelectedLabel(SubMenu);
            const auto& Subs=Categories[CorrectTop].SubCategoryViewModels;
            int Match=-1;
            for(int i=0;!Label.empty() && i<Subs.Num();++i) if(Subs[i].SubCategoryName.ToString()==Label) {
                if(Match>=0) {Match=-1;break;}
                Match=i;
            }
            if(Match>=0) SubMenu->CurrentPageIndex=Match;
        }
    }
    if(Log) Log("[rams-store-pilot] content top=%d->%d sub=%d->%d\n",SavedTop,TopMenu->CurrentPageIndex,SavedSub,SubMenu?SubMenu->CurrentPageIndex:0);
    OriginalContent(Screen);
    TopMenu->CurrentPageIndex=SavedTop;
    if(SubMenu) SubMenu->CurrentPageIndex=SavedSub;
}
inline void Uninstall(uintptr_t Base) {
    for(const uintptr_t Offset:{uintptr_t{0x198D210},uintptr_t{0x198CB00}}) {
        void* Target=reinterpret_cast<void*>(Base+Offset);MH_DisableHook(Target);MH_RemoveHook(Target);
    }
}
inline bool Install(uintptr_t Base,void(*Logger)(const char*,...)) {
    Log=Logger;
    void* A=reinterpret_cast<void*>(Base+0x198D210);
    void* B=reinterpret_cast<void*>(Base+0x198CB00);
    if(MH_CreateHook(A,reinterpret_cast<void*>(&Top),reinterpret_cast<void**>(&OriginalTop))!=MH_OK) return false;
    if(MH_CreateHook(B,reinterpret_cast<void*>(&Content),reinterpret_cast<void**>(&OriginalContent))!=MH_OK) {MH_RemoveHook(A);return false;}
    if(MH_EnableHook(A)!=MH_OK || MH_EnableHook(B)!=MH_OK) {MH_DisableHook(A);MH_DisableHook(B);return false;}
    return true;
}
}
