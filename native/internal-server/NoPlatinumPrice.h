#pragma once
#include <chrono>
#include <string>
#include <unordered_map>
#include <vector>

#include "SDK.hpp"

// Offers with no Platinum price (the Middleman's weekly Aetherdust cells) reach the 1.4.4 client
// with PlatPrice -1, and three widgets still show a Platinum option for them: "-1" on the cell in
// the offer grid, in the cell tooltip, and as a "-1 or ..." button in the purchase pop-up. Hide the
// Platinum part of each whenever its price is below zero; anything really sold for Platinum keeps it.
// The blueprints re-show these while open, so seen widgets are re-checked a few times a second.
namespace NoPlatinumPrice {

enum class EKind : uint8_t { None, Popup, GridItem, Tooltip };

struct FSeen {
    SDK::UObject* Object;
    int32_t Index;
    EKind Kind;
};

inline std::unordered_map<SDK::UClass*, EKind> Kinds;
inline std::vector<FSeen> Seen;
inline std::chrono::steady_clock::time_point NextCheck{};

inline EKind KindOf(SDK::UClass* Class) {
    auto It = Kinds.find(Class);
    if (It != Kinds.end())
        return It->second;

    const std::string Name = Class->GetName();
    const EKind Kind = Name == "w_popup_purchase_confirm_C" ? EKind::Popup
        : Name == "w_catalog_GridCellItem_C" ? EKind::GridItem
        : Name == "w_CellItem_tooltip_bpw_C" ? EKind::Tooltip
        : EKind::None;
    Kinds.emplace(Class, Kind);
    return Kind;
}

// A blueprint variable's offset by name (cached per class), or -1.
inline std::unordered_map<SDK::UClass*, std::unordered_map<std::string, int32_t>> Offsets;

inline int32_t OffsetOf(SDK::UClass* Class, const char* Name) {
    auto& ForClass = Offsets[Class];
    auto It = ForClass.find(Name);
    if (It != ForClass.end())
        return It->second;

    int32_t Offset = -1;
    for (auto* Struct = static_cast<SDK::UStruct*>(Class); Struct && Offset < 0; Struct = Struct->SuperStruct) {
        for (SDK::FField* Field = Struct->ChildProperties; Field; Field = Field->Next) {
            if (Field->Name.ToString() == Name) {
                Offset = static_cast<SDK::FProperty*>(Field)->Offset;
                break;
            }
        }
    }
    ForClass.emplace(Name, Offset);
    return Offset;
}

// A blueprint variable by name: its address in Object, or nullptr.
template <typename T>
inline T* Member(SDK::UObject* Object, const char* Name) {
    const int32_t Offset = OffsetOf(Object->Class, Name);
    return Offset < 0 ? nullptr : reinterpret_cast<T*>(reinterpret_cast<uint8_t*>(Object) + Offset);
}

template <typename T>
inline T* Widget(SDK::UObject* Object, const char* Name) {
    T** Slot = Member<T*>(Object, Name);
    return Slot ? *Slot : nullptr;
}

// UWidget::SetVisibility / GetParent through ProcessEvent (the UMG function bodies are not built into this DLL).
inline void Collapse(SDK::UWidget* Target) {
    if (!Target || Target->Visibility == SDK::ESlateVisibility::Collapsed)
        return;
    static SDK::UFunction* SetVisibility = nullptr;
    if (!SetVisibility)
        SetVisibility = Target->Class->GetFunction("Widget", "SetVisibility");
    if (!SetVisibility)
        return;
    struct { SDK::ESlateVisibility InVisibility; } Params{ SDK::ESlateVisibility::Collapsed };
    Target->ProcessEvent(SetVisibility, &Params);
}

inline SDK::UWidget* ParentOf(SDK::UWidget* Target) {
    if (!Target)
        return nullptr;
    static SDK::UFunction* GetParent = nullptr;
    if (!GetParent)
        GetParent = Target->Class->GetFunction("Widget", "GetParent");
    if (!GetParent)
        return nullptr;
    struct { SDK::UWidget* ReturnValue; } Params{ nullptr };
    Target->ProcessEvent(GetParent, &Params);
    return Params.ReturnValue;
}

// Center a widget in its vertical box (VerticalBoxSlot::SetHorizontalAlignment through ProcessEvent).
inline void CenterInVerticalBox(SDK::UWidget* Target) {
    if (!Target || !Target->Slot || !Target->Slot->IsA(SDK::UVerticalBoxSlot::StaticClass()))
        return;
    auto* BoxSlot = static_cast<SDK::UVerticalBoxSlot*>(Target->Slot);
    if (BoxSlot->HorizontalAlignment == SDK::EHorizontalAlignment::HAlign_Center)
        return;
    static SDK::UFunction* SetHorizontalAlignment = nullptr;
    if (!SetHorizontalAlignment)
        SetHorizontalAlignment = BoxSlot->Class->GetFunction("VerticalBoxSlot", "SetHorizontalAlignment");
    if (!SetHorizontalAlignment)
        return;
    struct { SDK::EHorizontalAlignment InHorizontalAlignment; } Params{ SDK::EHorizontalAlignment::HAlign_Center };
    BoxSlot->ProcessEvent(SetHorizontalAlignment, &Params);
}

inline bool ShowsNoPrice(SDK::UTextBlock* Text) {
    if (!Text)
        return false;
    const SDK::FText& Value = Text->Text;
    return Value.TextData && Value.ToString() == "-1";
}

inline void Apply(const FSeen& Entry) {
    SDK::UObject* Object = Entry.Object;
    switch (Entry.Kind) {
    case EKind::Popup: {
        // PrimaryValue is the Platinum price of the button left of "or"
        const int32_t* Price = Member<int32_t>(Object, "PrimaryValue");
        if (Price && *Price < 0) {
            Collapse(Widget<SDK::UWidget>(Object, "PrimaryButton_Overlay"));
            Collapse(Widget<SDK::UWidget>(Object, "txt_SecondaryButtonActive"));
        }
        break;
    }
    case EKind::GridItem: {
        // txt_value0 and its icon share one box in PricesBox: the Platinum price. With it gone, center
        // the price that is left under the cell.
        const int32_t* Price = Member<int32_t>(Object, "PlatValue");
        if (Price && *Price < 0) {
            Collapse(ParentOf(Widget<SDK::UWidget>(Object, "txt_value0")));
            CenterInVerticalBox(Widget<SDK::UWidget>(Object, "PricesBox"));
        }
        break;
    }
    case EKind::Tooltip:
        if (ShowsNoPrice(Widget<SDK::UTextBlock>(Object, "Txt_PlatinumCost"))) {
            Collapse(Widget<SDK::UWidget>(Object, "CurrencyImage"));
            Collapse(Widget<SDK::UWidget>(Object, "Txt_PlatinumCost"));
        }
        break;
    default:
        break;
    }
}

// Call after every client ProcessEvent.
inline void OnProcessEvent(SDK::UObject* Object) {
    if (Object && !Object->IsDefaultObject()) {
        const EKind Kind = KindOf(Object->Class);
        if (Kind != EKind::None) {
            bool Known = false;
            for (const FSeen& Entry : Seen)
                Known = Known || Entry.Object == Object;
            if (!Known) {
                if (Seen.size() >= 128)
                    Seen.erase(Seen.begin());
                Seen.push_back({ Object, Object->Index, Kind });
            }
        }
    }

    const auto Now = std::chrono::steady_clock::now();
    if (Seen.empty() || Now < NextCheck)
        return;
    NextCheck = Now + std::chrono::milliseconds(200);

    // Drop widgets whose slot no longer holds them (destroyed), then re-apply to the rest.
    std::erase_if(Seen, [](const FSeen& Entry) { return SDK::UObject::GObjects->GetByIndex(Entry.Index) != Entry.Object; });
    for (const FSeen& Entry : Seen)
        Apply(Entry);
}

}
