#include "../ServerRpcGuard.h"
#include <cassert>
#include <iostream>
#include <string>

int main() {
    for (const auto name : BlockedServerRpcs) {
        assert(IsBlockedServerRpc(name));
        assert(!IsBlockedServerRpc(std::string(name) + "Suffix"));
    }
    for (const auto name : {
        "Function Archon.ArchonCombatComponent.ServerStartCombo",
        "Function Archon.ArchonCombatComponent.ServerEndCombo",
        "Function Archon.ArchonInventory.ServerSpeedUpExchange",
        "Function Archon.ArchonTimeTranslationComponent.ServerQueryTime",
        "Function GameplayAbilities.AbilitySystemComponent.ServerTryActivateAbility",
        "Function GameplayAbilities.AbilitySystemComponent.ServerTryActivateAbilityWithEventData",
        "Function Engine.Character.ServerMove",
        "Function Engine.Character.ClientAdjustPosition",
        "Function monsterpart_bpc.monsterpart_bpc_C.AuthHandleBreak",
        "Function monsterpart_bpc.monsterpart_bpc_C.OnRep_CurrentBreakStage",
        "Function monsterpart_bpc.monsterpart_bpc_C.OnRep_PartSevered",
        "Function Archon.MonsterPartActor.OnRep_Health",
        "Function Other.ArchonPlayerController.ServerCheat",
        "", "ServerCheat"
    }) assert(!IsBlockedServerRpc(name));
    std::cout << "server RPC guard: privileged operations blocked; gameplay passes\n";
}
