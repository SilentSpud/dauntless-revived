#include "networking.h"

#include <iostream>
#include "ChannelLookup.h"
#include "OwnerRelevancy.h"
#include "ReplicationCadence.h"
#include <fstream>
#include <filesystem>
#include "NativeDiagnostics.h"
#include "MinHook/MinHook.h"

using namespace SDK;

extern "C" {
    __declspec(dllexport) volatile unsigned long long DR_ActorReplicationAttempts = 0;
    __declspec(dllexport) volatile unsigned long long DR_OwnerReplicationSkipped = 0;
    __declspec(dllexport) volatile unsigned long long DR_ActorUpdatesDeferred = 0;
    __declspec(dllexport) volatile unsigned long long DR_ChannelAssignmentTrackingEnabled = 0;
}

namespace Networking {
    UNetDriver* NetDriver = nullptr;
    static uintptr_t BaseAddress = 0x0;

    static std::vector<AActor*> BuildConsiderList(UWorld* World, UNetDriver* Driver) {
        std::vector<AActor*> Actors = std::vector<AActor*>();
        static ReplicationCadence Cadence;
        static UWorld* PreviousWorld = nullptr;
        if (PreviousWorld != World) { Cadence.Clear();PreviousWorld = World; }
        const double Now = GetTickCount64() / 1000.0;
        Cadence.Prune(Now);

        for (ULevel* Level : World->Levels) {
            if (!Level) continue;
            for (AActor* Actor : Level->Actors) {
                if (!Actor)
                    continue;

                if (Actor->RemoteRole == ENetRole::ROLE_None)
                    continue;

                if (Actor->bActorIsBeingDestroyed)
                    continue;

                if (!reinterpret_cast<UWorld * (*)(AActor*)>(*(void**)((uintptr_t)Actor->VTable + 0x150))(Actor)) {
                    continue;
                }

                const bool Urgent = Actor->bReplicateMovement || Actor->bTearOff || Actor->bNetTemporary ||
                    Actor->IsA(APawn::StaticClass()) || Actor->IsA(AController::StaticClass());
                if (!Cadence.Due(Actor, Actor->Index, Now, Actor->NetUpdateFrequency, Urgent)) {
                    ++DR_ActorUpdatesDeferred;
                    continue;
                }

                reinterpret_cast<void(*)(AActor*, UNetDriver*)>(BaseAddress + 0x306B150)(Actor, Driver);

                Actors.push_back(Actor);
            }
        }

        /*
        for (int i = 0; i < SDK::UObject::GObjects->Num(); i++)
        {
            SDK::UObject* Obj = SDK::UObject::GObjects->GetByIndex(i);

            if (!Obj)
                continue;

            if (Obj->IsDefaultObject())
                continue;

            if (Obj->IsA(SDK::AActor::StaticClass()))
            {
                AActor* Actor = (AActor*)Obj;

                if (Actor->RemoteRole == ENetRole::ROLE_None)
                    continue;

                if (Actor->bActorIsBeingDestroyed)
                    continue;

                if (!reinterpret_cast<UWorld * (*)(AActor*)>(*(void**)((uintptr_t)Actor->VTable + 0x150))(Actor)) {
                    continue;
                }
                
                reinterpret_cast<void(*)(AActor*, UNetDriver*)>(BaseAddress + 0x306B150)(Actor, Driver);

                Actors.push_back(Actor);
            }
        }
        */

        return Actors;
    }

    using ActorChannelIndex = ChannelIndex<AActor>;

    static AActor* ChannelActor(UChannel* Channel) {
        return Channel && Channel->Class == UActorChannel::StaticClass()
            ? static_cast<UActorChannel*>(Channel)->Actor : nullptr;
    }

    // Only scopes on the current game thread receive assignment notifications.
    // Other connections get a fresh index when their replication pass starts.
    // A linked scope also preserves notification delivery during reentrant calls.
    struct ChannelTrackingScope;
    static thread_local ChannelTrackingScope* ActiveChannelTracking = nullptr;

    struct ChannelTrackingScope {
        UNetConnection* Connection;
        ActorChannelIndex& Index;
        ChannelTrackingScope* Previous;

        ChannelTrackingScope(UNetConnection* InConnection, ActorChannelIndex& InIndex)
            : Connection(InConnection), Index(InIndex), Previous(ActiveChannelTracking) {
            ActiveChannelTracking = this;
        }
        ~ChannelTrackingScope() { ActiveChannelTracking = Previous; }
        ChannelTrackingScope(const ChannelTrackingScope&) = delete;
        ChannelTrackingScope& operator=(const ChannelTrackingScope&) = delete;
    };

    static void NoteActorAssignment(UNetConnection* Connection, AActor* Actor, bool Completed = false) {
        for (auto* Scope = ActiveChannelTracking; Scope; Scope = Scope->Previous)
            if (Scope->Connection == Connection) {
                if (Completed) Scope->Index.AssignmentCompleted(Actor, Connection->OpenChannels.Num(),
                    [Connection](int i) { return Connection->OpenChannels[i]; }, ChannelActor);
                else Scope->Index.ActorAssigned(Actor);
            }
    }

    using SetChannelActorFn = void(*)(UActorChannel*, AActor*, unsigned int);
    static void* OriginalSetChannelActor = nullptr;

    static void SetChannelActorHook(UActorChannel* Channel, AActor* Actor, unsigned int Flags) {
        // In 1.4.4, Connection is at 0x28 and SetChannelActor (0x3283450)
        // assigns Actor at 0x70. Use the same signature as our existing call.
        // Capture the owner while Channel is live; never dereference Channel
        // after the engine call, which may run callbacks or release channels.
        auto* Connection = Channel ? Channel->Connection : nullptr;
        NoteActorAssignment(Connection, Actor);
        reinterpret_cast<SetChannelActorFn>(OriginalSetChannelActor)(Channel, Actor, Flags);
        // A reentrant lookup can rebuild while the assignment is in progress.
        // Re-notify on return so it cannot erase knowledge of the new actor.
        NoteActorAssignment(Connection, Actor, true);
    }

    void InitChannelTracking(uintptr_t ImageBase) {
        auto* Target = reinterpret_cast<void*>(ImageBase + 0x3283450);
        const auto Created = MH_CreateHook(Target, SetChannelActorHook, &OriginalSetChannelActor);
        const auto Enabled = Created == MH_OK ? MH_EnableHook(Target) : Created;
        if (Enabled == MH_OK) {
            DR_ChannelAssignmentTrackingEnabled = 1;
            std::cout << "Actor channel assignment tracking enabled" << std::endl;
        } else {
            if (Created == MH_OK) MH_RemoveHook(Target);
            std::cerr << "Actor channel assignment tracking unavailable (" << Enabled
                << "); using conservative channel lookup" << std::endl;
        }
    }

    static ActorChannelIndex IndexActorChannels(UNetConnection* Connection) {
        return IndexChannels<AActor>(Connection->OpenChannels.Num(),
            [Connection](int i) { return Connection->OpenChannels[i]; }, ChannelActor,
            DR_ChannelAssignmentTrackingEnabled != 0);
    }

    static UActorChannel* GetActorChannelForConnectionAndActor(UNetConnection* Connection, AActor* Actor, ActorChannelIndex& Index) {
        return static_cast<UActorChannel*>(FindChannel(Actor, Index, Connection->OpenChannels.Num(),
            [Connection](int i) { return Connection->OpenChannels[i]; }, ChannelActor));
    }

    bool Listen(UEngine* Engine, int Port) {
        if (!Engine || !UWorld::GetWorld()) return false;
        NetDriver = nullptr;
        BaseAddress = (uintptr_t)GetModuleHandleA(nullptr);

        FName GameNetDriver = UKismetStringLibrary::Conv_StringToName(L"GameNetDriver");

        std::cout << "Net driver create: " << (int)reinterpret_cast<uint8_t(*)(UEngine*, void*, FName, FName)>(BaseAddress + 0x371A5E0)(Engine, UWorld::GetWorld(), GameNetDriver, GameNetDriver) << std::endl;

        for (int i = 0; i < SDK::UObject::GObjects->Num(); i++)
        {
            SDK::UObject* Obj = SDK::UObject::GObjects->GetByIndex(i);

            if (!Obj)
                continue;

            if (Obj->IsDefaultObject())
                continue;

            if (Obj->IsA(SDK::UNetDriver::StaticClass()))
            {
                NetDriver = (UNetDriver*)Obj;
                break;
            }
        }

        if (!NetDriver) {
            std::cerr << "Net driver creation failed" << std::endl;
            return false;
        }
        std::cout << NetDriver->GetFullName() << std::endl;

        reinterpret_cast<void(*)(UNetDriver*, UWorld*)>(BaseAddress + 0x3491890)(NetDriver, UWorld::GetWorld());

        FURL url = FURL();

        url.Port = Port;

        FString empy = FString();

        const bool Listening = (*(reinterpret_cast<bool(**)(UNetDriver*, void*, FURL*, bool, FString*)>(*(__int64*)NetDriver + 0x280)))(NetDriver, (void*)UWorld::GetWorld()->NetworkNotify, &url, false, &empy);
        std::cout << "Listen Status: " << Listening << std::endl;

        reinterpret_cast<void(*)(UNetDriver*, UWorld*)>(BaseAddress + 0x3491890)(NetDriver, UWorld::GetWorld());

        UWorld::GetWorld()->NetDriver = NetDriver;
        // A unique per-launch marker lets deployment wait for the actual listening world.
        // Never include credentials or player identities in this file.
        wchar_t ReadyPath[32768] = {};
        const DWORD Length = GetEnvironmentVariableW(L"DR_SERVER_READY_FILE", ReadyPath, 32768);
        if (Listening && Length > 0 && Length < 32768) {
            std::ofstream Ready{std::filesystem::path(ReadyPath)};
            Ready << GetCurrentProcessId() << ":" << Port;
        }

        return Listening;
    }

    void TickNetworking() {
        if (!NetDriver || !UWorld::GetWorld()) return;
        UWorld::GetWorld()->NetDriver = NetDriver;

        NetDriver->World = UWorld::GetWorld();

        static ULONGLONG NextDiagnostic = 0;
        static unsigned DiagnosticCount = 0;
        if (DiagnosticCount < 4096 && GetTickCount64() >= NextDiagnostic) {
            ++DiagnosticCount;
            NextDiagnostic = GetTickCount64() + 30000;
            int Ready = 0, Owned = 0;
            for (auto* Connection : NetDriver->ClientConnections) {
                if (!Connection) continue;
                Owned += Connection->OwningActor != nullptr;
                Ready += *(uint32_t*)((uintptr_t)Connection + 0x134) == 3;
            }
            char Line[512] = {};
            int Size = sprintf_s(Line, "net driver=%s connections=%d owned=%d open=%d\n",
                NetDriver->GetName().c_str(), NetDriver->ClientConnections.Num(), Owned, Ready);
            if (Size > 0) DR_WriteDiagnostic(Line, Size);
        }

        static FName name = FName();
        static bool nameInit = false;

        if (!nameInit) {
            nameInit = true;
            name = UKismetStringLibrary::Conv_StringToName(L"Actor");
        }

        ++ * (uint32_t*)((uintptr_t)NetDriver + 0x2AC);

        bool HasReadyConnection = false;
        for (UNetConnection* Connection : NetDriver->ClientConnections) {
            if (Connection && Connection->OwningActor && *(uint32_t*)((uintptr_t)Connection + 0x134) == 3) {
                HasReadyConnection = true;
                break;
            }
        }
        if (!HasReadyConnection) return;

        std::vector<AActor*> Actors = BuildConsiderList(UWorld::GetWorld(), NetDriver);

        for (UNetConnection* Connection : NetDriver->ClientConnections) {
            if (!Connection || !Connection->OwningActor || *(uint32_t*)((uintptr_t)Connection + 0x134) != 3)
                continue;

            auto Channels = IndexActorChannels(Connection);
            ChannelTrackingScope TrackAssignments(Connection, Channels);
            for (AActor* Actor : Actors) {
                // The engine's normal relevancy pass is replaced by this loop. Preserve
                // owner-only isolation rather than opening private actors on every client.
                auto* Pawn = Connection->PlayerController ? Connection->PlayerController->Pawn : nullptr;
                if (Actor->bOnlyRelevantToOwner && !Actor->bAlwaysRelevant &&
                    !(Pawn && Actor->Instigator == Pawn) &&
                    !IsConnectionOwner(Actor, Connection->OwningActor,
                        static_cast<AActor*>(Pawn), [](AActor* Value) { return Value->Owner; })) {
                    ++DR_OwnerReplicationSkipped;
                    continue;
                }
                if (Actor->Class->CastFlags & EClassCastFlags::PlayerController) {
                    if (Actor != Connection->OwningActor) {
                        continue;
                    }
                    else {
                        Connection->ViewTarget = ((APlayerController*)Actor)->GetViewTarget();

                        reinterpret_cast<void(*)(APlayerController*)>(BaseAddress + 0x359F9D0)((APlayerController*)Actor);
                    }
                }

                //

                UActorChannel* ActorChannel = GetActorChannelForConnectionAndActor(Connection, Actor, Channels);

                if (!ActorChannel) {
                    ActorChannel = reinterpret_cast<UActorChannel * (*)(UNetConnection*, FName*, unsigned int, int)>(BaseAddress + 0x3449E10)(Connection, &name, 1 << 1, -1);

                    if (ActorChannel) {
                        reinterpret_cast<void(*)(UActorChannel*, AActor*, unsigned int)>(BaseAddress + 0x3283450)(ActorChannel, Actor, 0);
                    }
                }

                if (ActorChannel && ActorChannel->Actor) {
                    ++DR_ActorReplicationAttempts;
                    if (!(*(int*)((uintptr_t)ActorChannel + 0x90) & 2u)) {
                        *(int*)((uintptr_t)ActorChannel + 0x90) |= 2u;
                    }
                    if (reinterpret_cast<bool(*)(UActorChannel*)>(BaseAddress + 0x327E860)(ActorChannel)) {
                        //std::cout << ActorChannel->Actor->GetFullName() << std::endl;
                    }
                }
            }
        }
    }
}
