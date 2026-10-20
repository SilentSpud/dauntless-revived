#pragma once
#include <windows.h>

#include <array>
#include <chrono>
#include <cstring>
#include <map>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "SDK.hpp"
#include "MinHook/MinHook.h"
#include "DualSense.h"

// With a DualSense connected, the button prompts show PlayStation 5 buttons instead of Xbox ones.
// The game finds a key's prompt through UArchonUIBlueprintLibrary::GetKeyInfo, which reads the PC
// column of its key info table (UArchonGameInstance::InputKeyInfoTable): Xbox textures for the
// gamepad keys. The answer is changed on the way out, so prompts the game already cached change too,
// and the table itself is pointed at the same icons. Prompts keep their places and sizes. Widgets
// that show a gamepad texture directly get the PS5 icon by texture name, and the Controls screen
// shows its PlayStation page with a DualSense picture. The icons load from PS5Icons\ next to this
// DLL (installed with the client mods); without a DualSense everything stays as the game ships it.
//
// PS5 Button Icons and Controls by Zacksly, licensed under CC BY 3.0 (https://zacksly.itch.io).
namespace DualSenseIcons {

// A gamepad key (Unreal key name) and its PS5 icon.
inline constexpr std::array<std::pair<std::string_view, std::wstring_view>, 33> KeyIcons = { {
    { "Gamepad_FaceButton_Bottom", L"Cross" },
    { "Gamepad_FaceButton_Right", L"Circle" },
    { "Gamepad_FaceButton_Left", L"Square" },
    { "Gamepad_FaceButton_Top", L"Triangle" },
    { "Gamepad_LeftShoulder", L"L1" },
    { "Gamepad_RightShoulder", L"R1" },
    { "Gamepad_LeftTrigger", L"L2" },
    { "Gamepad_LeftTriggerAxis", L"L2" },
    { "Gamepad_RightTrigger", L"R2" },
    { "Gamepad_RightTriggerAxis", L"R2" },
    { "Gamepad_Special_Left", L"Touch Pad Press" },
    { "Gamepad_Special_Right", L"Options" },
    { "Gamepad_LeftThumbstick", L"Left Stick Click" },
    { "Gamepad_RightThumbstick", L"Right Stick Click" },
    { "Gamepad_DPad_Up", L"D-Pad Up" },
    { "Gamepad_DPad_Down", L"D-Pad Down" },
    { "Gamepad_DPad_Left", L"D-Pad Left" },
    { "Gamepad_DPad_Right", L"D-Pad Right" },
    { "Gamepad_LeftX", L"Left Stick Left-Right" },
    { "Gamepad_LeftY", L"Left Stick Up-Down" },
    { "Gamepad_Left2D", L"Left Stick All" },
    { "Gamepad_LeftStick_Up", L"Left Stick Up" },
    { "Gamepad_LeftStick_Down", L"Left Stick Down" },
    { "Gamepad_LeftStick_Left", L"Left Stick Left" },
    { "Gamepad_LeftStick_Right", L"Left Stick Right" },
    { "Gamepad_RightX", L"Right Stick Left-Right" },
    { "Gamepad_RightY", L"Right Stick Up-Down" },
    { "Gamepad_Right2D", L"Right Stick All" },
    { "Gamepad_RightStick_Up", L"Right Stick Up" },
    { "Gamepad_RightStick_Down", L"Right Stick Down" },
    { "Gamepad_RightStick_Left", L"Right Stick Left" },
    { "Gamepad_RightStick_Right", L"Right Stick Right" },
    { "Gamepad_Special_Left_X", L"Touch Pad Press" },
} };

// Some widgets show a gamepad texture directly (the consumables wheel shows the d-pad, the Controls
// screen's PlayStation page its PS4 buttons), not through GetKeyInfo: those get the PS5 icon by name.
inline constexpr std::array<std::pair<std::string_view, std::wstring_view>, 49> TextureIcons = { {
    // The Controls screen's PlayStation page picture: a DualSense with the page's lines.
    { "ui_playstation_controller", L"DualSense Layout" },
    { "PS4_Dpad", L"D-Pad" },
    { "PS4_Dpad_up", L"D-Pad Up" },
    { "PS4_Dpad_dn", L"D-Pad Down" },
    { "PS4_Dpad_L", L"D-Pad Left" },
    { "PS4_Dpad_R", L"D-Pad Right" },
    { "PS4_Dpad_TouchButton", L"Touch Pad Press" },
    { "PS4_Options", L"Options" },
    { "ico_sony_X", L"Cross" },
    { "ico_sony_circle", L"Circle" },
    { "ico_sony_square", L"Square" },
    { "ico_sony_triangle", L"Triangle" },
    { "ico_sony_L1", L"L1" },
    { "ico_sony_L2", L"L2" },
    { "ico_sony_L3", L"Left Stick Click" },
    { "ico_sony_R1", L"R1" },
    { "ico_sony_R2", L"R2" },
    { "ico_sony_R3", L"Right Stick Click" },
    { "Dpad_msft", L"D-Pad" },
    { "gamepad_AButton", L"Cross" },
    { "gamepad_BButton", L"Circle" },
    { "gamepad_XButton", L"Square" },
    { "gamepad_YButton", L"Triangle" },
    { "gamepad_LBButton", L"L1" },
    { "gamepad_RBButton", L"R1" },
    { "gamepad_LTButton", L"L2" },
    { "gamepad_RTButton", L"R2" },
    { "Button_Xbox_Select", L"Touch Pad Press" },
    { "Button_Xbox_Start", L"Options" },
    { "LeftStick_Click", L"Left Stick Click" },
    { "RightStick_Click", L"Right Stick Click" },
    { "ui_controller_xbox_dpad_up", L"D-Pad Up" },
    { "ui_controller_xbox_dpad_down", L"D-Pad Down" },
    { "ui_controller_xbox_dpad_left", L"D-Pad Left" },
    { "ui_controller_xbox_dpad_right", L"D-Pad Right" },
    { "ui_controller_left_stick_all", L"Left Stick All" },
    { "ui_controller_left_stick_up", L"Left Stick Up" },
    { "ui_controller_left_stick_down", L"Left Stick Down" },
    { "ui_controller_left_stick_left", L"Left Stick Left" },
    { "ui_controller_left_stick_right", L"Left Stick Right" },
    { "ui_controller_left_stick_leftright", L"Left Stick Left-Right" },
    { "ui_controller_left_stick_updown", L"Left Stick Up-Down" },
    { "ui_controller_right_stick_all", L"Right Stick All" },
    { "ui_controller_right_stick_up", L"Right Stick Up" },
    { "ui_controller_right_stick_down", L"Right Stick Down" },
    { "ui_controller_right_stick_left", L"Right Stick Left" },
    { "ui_controller_right_stick_right", L"Right Stick Right" },
    { "ui_controller_right_stick_leftright", L"Right Stick Left-Right" },
    { "ui_controller_right_stick_updown", L"Right Stick Up-Down" },
} };


using SoftTexture = SDK::TSoftObjectPtr<SDK::UTexture2D>;

struct Swap {
    SoftTexture* Field;
    alignas(SoftTexture) uint8_t Original[sizeof(SoftTexture)];
};

inline std::vector<Swap> Swapped;
inline SDK::UDataTable* AppliedTable = nullptr;
inline int32_t AppliedTableIndex = -1;
// Images given a PS5 icon: object index, the image, and the texture it had.
struct ImageSwap {
    int32_t Index;
    SDK::UImage* Image;
    SDK::UTexture2D* Original;
};
inline std::vector<ImageSwap> SwappedImages;
inline std::chrono::steady_clock::time_point NextScan{};
inline std::map<std::wstring_view, SDK::UTexture2D*> Loaded;
inline bool Applied = false;
inline std::chrono::steady_clock::time_point NextCheck{};

// Keeps a texture the game made for us out of garbage collection (EInternalObjectFlags::RootSet).
inline void AddToRoot(SDK::UObject* Object) {
    auto* Array = SDK::UObject::GObjects.operator->();
    const int32_t Index = Object->Index;
    auto* Chunk = Array->GetDecrytedObjPtr()[Index / SDK::TUObjectArray::ElementsPerChunk];
    auto* Item = reinterpret_cast<uint8_t*>(&Chunk[Index % SDK::TUObjectArray::ElementsPerChunk]);
    *reinterpret_cast<int32_t*>(Item + 0x8) |= (1 << 30);
}

inline std::wstring IconFolder() {
    HMODULE Self = nullptr;
    GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<LPCWSTR>(&IconFolder), &Self);
    wchar_t Path[MAX_PATH] = {};
    GetModuleFileNameW(Self, Path, MAX_PATH);
    std::wstring Folder(Path);
    Folder.resize(Folder.find_last_of(L"\\/") + 1);
    return Folder + L"PS5Icons\\";
}

// Makes every icon once (on the game thread) and keeps it.
inline void LoadAll(SDK::UWorld* World) {
    std::vector<std::wstring_view> All;
    for (const auto& [Key, Icon] : KeyIcons) All.push_back(Icon);
    for (const auto& [Texture, Icon] : TextureIcons) All.push_back(Icon);
    for (const std::wstring_view Icon : All) {
        if (Loaded.count(Icon)) continue;
        const std::wstring File = IconFolder() + std::wstring(Icon) + L".png";
        SDK::UTexture2D* Made = nullptr;
        if (GetFileAttributesW(File.c_str()) != INVALID_FILE_ATTRIBUTES)
            Made = SDK::UKismetRenderingLibrary::ImportFileAsTexture2D(World, SDK::FString(File.c_str()));
        if (Made) AddToRoot(Made);
        Loaded[Icon] = Made;
    }
}

inline SDK::UTexture2D* IconFor(const SDK::FName& KeyName) {
    const std::string Name = KeyName.ToString();
    for (const auto& [Key, Icon] : KeyIcons) {
        if (Key != Name) continue;
        const auto Found = Loaded.find(Icon);
        return Found == Loaded.end() ? nullptr : Found->second;
    }
    return nullptr;
}

inline void Point(SoftTexture* Field, SDK::UTexture2D* Icon) {
    auto Soft = SDK::UKismetSystemLibrary::Conv_ObjectToSoftObjectReference(Icon);
    std::memcpy(Field, &Soft, sizeof(SoftTexture));
}

inline SDK::UDataTable* KeyTable(SDK::UWorld* World) {
    SDK::UGameInstance* Instance = World ? World->OwningGameInstance : nullptr;
    if (!Instance || !Instance->IsA(SDK::UArchonGameInstance::StaticClass())) return nullptr;
    return static_cast<SDK::UArchonGameInstance*>(Instance)->InputKeyInfoTable;
}

inline void Apply(SDK::UWorld* World) {
    SDK::UDataTable* Table = KeyTable(World);
    if (!Table) return;
    AppliedTable = Table;
    AppliedTableIndex = Table->Index;
    LoadAll(World);
    auto& Map = Table->RowMap;
    for (int R = 0; R < Map.NumAllocated(); ++R) {
        if (!Map.IsValidIndex(R) || !Map[R].Value()) continue;
        SDK::UTexture2D* Icon = IconFor(Map[R].Key());
        if (!Icon) continue;
        auto* Row = reinterpret_cast<SDK::FKeyInfoTableData*>(Map[R].Value());
        for (SoftTexture* Field : { &Row->PCButton, &Row->XboxButton }) {
            Swap S{ Field };
            std::memcpy(S.Original, Field, sizeof(SoftTexture));
            Swapped.push_back(S);
            Point(Field, Icon);
        }
    }
    Applied = true;
}

inline SDK::UTexture2D* IconForTexture(const std::string& Name) {
    for (const auto& [Texture, Icon] : TextureIcons) {
        if (Texture != Name) continue;
        const auto Found = Loaded.find(Icon);
        return Found == Loaded.end() ? nullptr : Found->second;
    }
    return nullptr;
}

// UImage::SetBrushFromTexture through ProcessEvent (the UMG function bodies are not built into this DLL).
inline void SetImage(SDK::UImage* Image, SDK::UTexture2D* Texture) {
    static SDK::UFunction* Set = nullptr;
    if (!Set) Set = Image->Class->GetFunction("Image", "SetBrushFromTexture");
    if (!Set) return;
    struct { SDK::UTexture2D* Texture; bool bMatchSize; uint8_t Pad[7]; } Params{ Texture, false, {} };
    Image->ProcessEvent(Set, &Params);
}

// Finds images showing an Xbox texture directly and gives them the PS5 icon.
inline void ScanImages() {
    auto* Objects = SDK::UObject::GObjects.operator->();
    for (int32_t I = 0; I < Objects->Num(); ++I) {
        SDK::UObject* Object = Objects->GetByIndex(I);
        if (!Object || !Object->IsA(SDK::UImage::StaticClass()) || Object->IsDefaultObject()) continue;
        auto* Image = static_cast<SDK::UImage*>(Object);
        SDK::UObject* Shown = Image->Brush.ResourceObject;
        if (!Shown || !Shown->IsA(SDK::UTexture2D::StaticClass())) continue;
        SDK::UTexture2D* Icon = IconForTexture(Shown->GetName());
        if (!Icon) continue;
        SwappedImages.push_back({ I, Image, static_cast<SDK::UTexture2D*>(Shown) });
        SetImage(Image, Icon);
    }
}

// The Controls screen (w_options_gamepad_layout) has a whole page per platform, each with its own
// picture, lines and labels: Xbox, PS4 and Switch. With a DualSense, show the PlayStation page.
struct SwitcherSwap {
    int32_t Index;
    SDK::UWidgetSwitcher* Switcher;
    int32_t Original;
};
inline std::vector<SwitcherSwap> SwappedSwitchers;

inline SDK::UWidget* FindWidget(SDK::UWidget* W, std::string_view Name) {
    if (!W) return nullptr;
    if (W->GetName() == Name) return W;
    if (W->IsA(SDK::UPanelWidget::StaticClass())) {
        auto& Slots = static_cast<SDK::UPanelWidget*>(W)->Slots;
        for (int I = 0; I < Slots.Num(); ++I)
            if (Slots[I])
                if (SDK::UWidget* Found = FindWidget(Slots[I]->Content, Name)) return Found;
    }
    return nullptr;
}

inline void SetSwitcher(SDK::UWidgetSwitcher* Switcher, int32_t Index) {
    static SDK::UFunction* Set = nullptr;
    if (!Set) Set = Switcher->Class->GetFunction("WidgetSwitcher", "SetActiveWidgetIndex");
    if (!Set) return;
    struct { int32_t Index; } Params{ Index };
    Switcher->ProcessEvent(Set, &Params);
}

inline void ShowPlayStationLayout() {
    auto* Objects = SDK::UObject::GObjects.operator->();
    for (int32_t I = 0; I < Objects->Num(); ++I) {
        SDK::UObject* Object = Objects->GetByIndex(I);
        if (!Object || Object->IsDefaultObject() || !Object->IsA(SDK::UUserWidget::StaticClass())) continue;
        if (Object->Class->GetName() != "w_options_gamepad_layout_C") continue;
        auto* Tree = static_cast<SDK::UUserWidget*>(Object)->WidgetTree;
        if (!Tree || !Tree->RootWidget) continue;
        for (std::string_view Name : { "PlatformGamepadSwitcher", "PlatformMenuSwitcher" }) {
            SDK::UWidget* Found = FindWidget(Tree->RootWidget, Name);
            if (!Found || !Found->IsA(SDK::UWidgetSwitcher::StaticClass())) continue;
            auto* Switcher = static_cast<SDK::UWidgetSwitcher*>(Found);
            if (Switcher->ActiveWidgetIndex == 1) continue;
            SwappedSwitchers.push_back({ Switcher->Index, Switcher, Switcher->ActiveWidgetIndex });
            SetSwitcher(Switcher, 1);
        }
    }
}

inline void Restore() {
    for (const SwitcherSwap& S : SwappedSwitchers)
        if (SDK::UObject::GObjects->GetByIndex(S.Index) == S.Switcher) SetSwitcher(S.Switcher, S.Original);
    SwappedSwitchers.clear();
    if (AppliedTable && SDK::UObject::GObjects->GetByIndex(AppliedTableIndex) == AppliedTable)
        for (const Swap& S : Swapped) std::memcpy(S.Field, S.Original, sizeof(SoftTexture));
    AppliedTable = nullptr;
    AppliedTableIndex = -1;
    Swapped.clear();
    for (const ImageSwap& S : SwappedImages)
        if (SDK::UObject::GObjects->GetByIndex(S.Index) == S.Image) SetImage(S.Image, S.Original);
    SwappedImages.clear();
    Applied = false;
}

// UArchonUIBlueprintLibrary::GetKeyInfo (native, 1.4.4): every prompt goes through it.
using GetKeyInfoFn = bool(__fastcall*)(SDK::UObject*, const SDK::FKey*, SDK::FKeyInfo*, bool*, uint8_t, bool);
inline GetKeyInfoFn OrigGetKeyInfo = nullptr;
constexpr uintptr_t GetKeyInfoOffset = 0x1712EC0;

inline bool __fastcall GetKeyInfoHook(SDK::UObject* Context, const SDK::FKey* Key, SDK::FKeyInfo* KeyInfo, bool* ShowLabel, uint8_t PlatformOverride, bool SwapVirtual) {
    const bool Found = OrigGetKeyInfo(Context, Key, KeyInfo, ShowLabel, PlatformOverride, SwapVirtual);
    if (Found && Key && KeyInfo && Applied) {
        if (SDK::UTexture2D* Icon = IconFor(*reinterpret_cast<const SDK::FName*>(Key)))
            Point(&KeyInfo->Button, Icon);
    }
    return Found;
}

// Call from the client hooks, after MH_Initialize.
inline void Install(uintptr_t Base) {
    void* Target = reinterpret_cast<void*>(Base + GetKeyInfoOffset);
    if (MH_CreateHook(Target, reinterpret_cast<void*>(&GetKeyInfoHook), reinterpret_cast<void**>(&OrigGetKeyInfo)) == MH_OK)
        MH_EnableHook(Target);
}

// Call after every client ProcessEvent (the game thread, where textures can be made).
inline void OnProcessEvent() {
    const auto Now = std::chrono::steady_clock::now();
    if (Now < NextCheck) return;
    NextCheck = Now + std::chrono::milliseconds(250);

    if (Applied && KeyTable(SDK::UWorld::GetWorld()) != AppliedTable) Restore();
    const bool Want = DualSense::Read().Connected;
    if (Want && Applied && Now >= NextScan) {
        NextScan = Now + std::chrono::seconds(2);
        ScanImages();
        ShowPlayStationLayout();
    }
    if (Want == Applied) return;
    if (!Want) {
        Restore();
        return;
    }
    if (SDK::UWorld* World = SDK::UWorld::GetWorld()) Apply(World);
}

}
