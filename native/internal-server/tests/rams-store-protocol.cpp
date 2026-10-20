#include "../RamsStoreProtocol.h"
#include <cassert>
#include <iostream>
int main() {
 using namespace RamsStoreProtocol;
 assert(Requested(L"game.exe -RamsStore"));
 assert(Requested(L"game.exe -RamsStorePilot -windowed"));
 assert(!Requested(L"game.exe -RamsStoreDisabled"));
 assert(!Requested(L"game.exe address-RamsStore"));
 assert(!Requested(L"game.exe"));
 std::wstring Plain=L"http://host/product/sku/{sku_id}";
 Mark(L"StoreGetItemByIdEndpoint",Plain,false);assert(Plain.find(L"shop_protocol")==std::wstring::npos);
 Mark(L"StoreGetItemByIdEndpoint",Plain,true);assert(Plain.ends_with(L"?shop_protocol=rams-v1"));
 auto Once=Plain;Mark(L"StoreGetItemByIdEndpoint",Plain,true);assert(Plain==Once);
 std::wstring Query=L"http://host/notification/{currency}?token={purchase_token}";
 Mark(L"StorePurchaseItemConfirmEndpoint",Query,true);assert(Query.ends_with(L"&shop_protocol=rams-v1"));
 std::wstring Other=L"http://host/balance";Mark(L"StoreBalancesEndpoint",Other,true);assert(Other==L"http://host/balance");
 std::cout<<"Rams store activation and protocol tests passed\n";
}
