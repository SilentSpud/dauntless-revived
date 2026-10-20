#pragma once
#include <string>
#include <string_view>
#include <cwctype>
namespace RamsStoreProtocol {
inline bool Requested(std::wstring_view Args) {
    for(const std::wstring_view Flag : {L"-RamsStore",L"-RamsStorePilot"}) {
        size_t At=0;
        while((At=Args.find(Flag,At))!=std::wstring_view::npos) {
            if((At==0 || std::iswspace(Args[At-1])) && (At+Flag.size()==Args.size() || std::iswspace(Args[At+Flag.size()]))) return true;
            At+=Flag.size();
        }
    }
    return false;
}
inline void Mark(std::wstring_view Key,std::wstring& Endpoint,bool Enabled) {
    if(!Enabled || Endpoint.find(L"shop_protocol=rams-v1")!=std::wstring::npos) return;
    if(Key!=L"StoreGetItemByTagEndpoint" && Key!=L"StoreGetItemByIdEndpoint" && Key!=L"StorePurchaseItemEndpoint" && Key!=L"StorePurchaseItemConfirmEndpoint") return;
    Endpoint += Endpoint.find(L'?')==std::wstring::npos ? L"?shop_protocol=rams-v1" : L"&shop_protocol=rams-v1";
}
}
