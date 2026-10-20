#pragma once
#include <array>
#include <string_view>

// Exact SDK names only: ordinary combat, movement and cell-fusion RPCs must pass.
// Blocking a privileged operation is not proof that a particular player cheated.
inline constexpr std::array<std::string_view, 10> BlockedServerRpcs = {
    "Function Archon.ArchonPlayerController.ServerCheat",
    "Function Archon.ArchonPlayerController.ServerArchonCheat",
    "Function Archon.ArchonPlayerController.ServerDebugEventEndActivity",
    "Function Archon.ArchonPlayerController.ServerDebugEventSetCurrentTimer",
    "Function Archon.ArchonPlayerController.ServerDebugEventSetNextActivity",
    "Function Archon.ArchonPlayerController.ServerDebugEventStartGameMode",
    "Function Archon.ArchonLoadout.ServerEquipLoadoutFromJsonCheat",
    "Function Archon.CooldownComponent.ServerDebugSetCooldown",
    "Function Archon.SchedulerComponent.ServerDebugSetSchedulerDisabled",
    "Function Archon.ArchonPlayerController.ServerAddDebugProperty",
};

inline bool IsBlockedServerRpc(std::string_view name) {
    for (const auto blocked : BlockedServerRpcs)
        if (name == blocked) return true;
    return false;
}
