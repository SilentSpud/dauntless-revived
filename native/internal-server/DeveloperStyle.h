#pragma once
#include <string>
#include <unordered_map>
#include "SDK.hpp"
#include "SDK/w_ChatMessage_bpw_classes.hpp"

// Cosmetic only: these names confer no authorization or account privileges.
// Use authenticated sender metadata, never search a chat message body for a staff name.
namespace DeveloperStyle {
inline bool IsDeveloper(const SDK::FString& Name) {
    const auto Value=Name.ToString();
    return Value=="TFBT" || Value=="TFBTA" || Value=="ZFXSTATIC";
}
inline void Text(SDK::UTextBlock* Block,const std::wstring& Value) {
    if(!Block)return;
    auto* Function=Block->Class->GetFunction("TextBlock","SetText");
    if(!Function)return;
    struct {SDK::FText InText;} Params{SDK::UKismetTextLibrary::Conv_StringToText(SDK::FString(Value.c_str()))};
    Block->ProcessEvent(Function,&Params);
}
inline void Purple(SDK::UTextBlock* Block) {
    if(!Block)return;
    auto* Function=Block->Class->GetFunction("TextBlock","SetColorAndOpacity");
    if(!Function)return;
    SDK::FSlateColor Color{};
    Color.SpecifiedColor={0.68f,0.30f,1.0f,1.0f};
    Color.ColorUseRule=static_cast<SDK::ESlateColorStylingMode>(0);
    Block->ProcessEvent(Function,&Color);
}
inline void Bold(SDK::UTextBlock* Block) {
    if(!Block)return;
    auto* Function=Block->Class->GetFunction("TextBlock","SetFont");
    if(!Function)return;
    auto Font=Block->Font;
    Font.TypefaceFontName=SDK::UKismetStringLibrary::Conv_StringToName(SDK::FString(L"Bold"));
    Block->ProcessEvent(Function,&Font);
}
inline void OnProcessEvent(SDK::UObject* Object,SDK::UFunction* Function) {
    if(!Object || !Object->Class || !Function)return;
    static std::unordered_map<SDK::UClass*,int> Kinds;
    auto It=Kinds.find(Object->Class);
    if(It==Kinds.end()) {
        const auto Name=Object->Class->GetName();
        It=Kinds.emplace(Object->Class,Name=="w_identity_nameplate_bpw_C"?1:Name=="w_ChatMessage_bpw_C"?2:0).first;
    }
    if(!It->second)return;
    const auto Name=Function->GetName();
    if(It->second==1 && Name=="UpdateView") {
        auto* Plate=static_cast<SDK::UIdentityNameplateWidget*>(Object);
        if(!IsDeveloper(Plate->NameplateViewModel.PlayerName))return;
        Text(Plate->TitleTextBlock,L"[ Server Developer ]");
        Purple(Plate->TitleTextBlock);Bold(Plate->NameTextBlock);
        if(Plate->TitleTextBlock) {
            auto* Show=Plate->TitleTextBlock->Class->GetFunction("Widget","SetVisibility");
            SDK::ESlateVisibility Value=SDK::ESlateVisibility::SelfHitTestInvisible;
            if(Show)Plate->TitleTextBlock->ProcessEvent(Show,&Value);
        }
    }else if(It->second==2 && Name=="Setup_Message") {
        auto* Message=static_cast<SDK::Uw_ChatMessage_bpw_C*>(Object);
        SDK::FChatUserInfo Sender{},Recipient{},Context{};SDK::FText Body{},Room{};
        SDK::FDateTime Time{};bool Self=false;SDK::TSoftObjectPtr<SDK::UTexture2D> Emoji{};
        SDK::UChatClient::BreakBPChatClientMessage(Message->Message,&Sender,&Recipient,&Context,&Body,&Room,&Time,&Self,&Emoji);
        if(!IsDeveloper(Sender.Name) || !Message->Content || !Message->Content->Text.TextData)return;
        const auto* Chars=Message->Content->Text.GetStringRef().CStr();
        if(!Chars)return;
        const std::wstring Original(Chars);
        if(!Original.starts_with(L"[ Server Developer ] "))Text(Message->Content,L"[ Server Developer ] "+Original);
        Purple(Message->Content);Bold(Message->Content);
    }
}
}

