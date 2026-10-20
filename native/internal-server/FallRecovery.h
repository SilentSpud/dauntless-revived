#pragma once

#include "FallRecoveryPolicy.h"
#include "SDK.hpp"
#include <array>
#include <map>
#include <set>
#include <iostream>

namespace FallRecovery {
using namespace SDK;

struct Anchor {
    FTransform Transform{};
    FRotator Rotation{};
    ECollisionEnabled Collision = ECollisionEnabled::QueryAndPhysics;
};

struct PlayerState {
    APlayerController* Controller = nullptr;
    ABP_PlayerCharacter_C* Pawn = nullptr;
    Policy Watchdog;
    std::array<Anchor, 4> Ground{};
    size_t Count = 0;
    Anchor Spawn{};
    bool HasSpawn = false;
};

struct PendingStart {
    APlayerController* Controller = nullptr;
    Anchor Position{};
    double Expires = 0.0;
};

inline std::map<UNetConnection*, PlayerState> Players;
inline std::map<AArchonPlayerState*, PendingStart> Starts;
inline UWorld* CurrentWorld = nullptr;
inline ULevel* CurrentLevel = nullptr;
inline double Clock = 0.0;
inline double PollSeconds = 0.0;

inline UWorld* ObjectWorld(UObject* Object) {
    for (UObject* Outer = Object; Outer; Outer = Outer->Outer)
        if (Outer->IsA(UWorld::StaticClass()))
            return static_cast<UWorld*>(Outer);
    return nullptr;
}

inline void UseWorld(UWorld* World) {
    ULevel* Level = World ? World->PersistentLevel : nullptr;
    if (World != CurrentWorld || Level != CurrentLevel) {
        Players.clear();
        Starts.clear();
        Clock = PollSeconds = 0.0;
        CurrentWorld = World;
        CurrentLevel = Level;
    }
}

inline bool ValidAnchor(const Anchor& Point) {
    const auto& T = Point.Transform;
    const auto& Q = T.Rotation;
    const double Length = Q.X * Q.X + Q.Y * Q.Y + Q.Z * Q.Z + Q.W * Q.W;
    return FinitePosition(T.Translation) && FinitePosition(T.Scale3D)
        && T.Scale3D.X > 0 && T.Scale3D.Y > 0 && T.Scale3D.Z > 0
        && std::isfinite(Length) && Length > 0.5 && Length < 1.5
        && std::isfinite(Point.Rotation.Pitch) && std::isfinite(Point.Rotation.Yaw)
        && std::isfinite(Point.Rotation.Roll);
}

// Called only for the start actually selected by Phoenix's group/slot-aware selector.
// Never substitute the first global PlayerStart, or a start from another world.
inline void RememberStart(AArchonPlayerState* State, AArchonPlayerStart* Start) {
    if (!State || !Start || Start->IsActorBeingDestroyed())
        return;
    UWorld* World = ObjectWorld(State);
    if (!World || ObjectWorld(Start) != World)
        return;
    UseWorld(World);
    Anchor Point{Start->GetTransform(), Start->K2_GetActorRotation()};
    if (ValidAnchor(Point)) {
        auto* Controller = State->GetOwner();
        Starts[State] = {Controller && Controller->IsA(APlayerController::StaticClass())
            ? static_cast<APlayerController*>(Controller) : nullptr, Point, Clock + 120.0};
    }
}

inline bool FloorAt(ABP_PlayerCharacter_C* Player, const FVector& Location, double Maximum) {
    if (!FinitePosition(Location) || !Player->CharacterMovement || !Player->CapsuleComponent)
        return false;
    FFindFloorResult Floor{};
    const float Radius = Player->CapsuleComponent->GetScaledCapsuleRadius();
    if (!std::isfinite(Radius) || Radius <= 0.0f)
        return false;
    // Compute a fresh floor sweep: CurrentFloor can describe the old location after teleporting.
    Player->CharacterMovement->K2_ComputeFloorDist(Location, 50.0f, 50.0f, Radius, &Floor);
    const float Distance = Floor.bLineTrace ? Floor.LineDist : Floor.FloorDist;
    return ValidFloor(Floor.bBlockingHit, Floor.bWalkableFloor,
        Floor.HitResult.bStartPenetrating, Distance, Maximum);
}

inline bool CapsuleFits(ABP_PlayerCharacter_C* Player, const FVector& Location) {
    auto* Capsule = Player->CapsuleComponent;
    const float Radius = Capsule->GetScaledCapsuleRadius();
    const float Height = Capsule->GetScaledCapsuleHalfHeight();
    if (!std::isfinite(Radius) || !std::isfinite(Height) || Radius <= 1.0f || Height < Radius)
        return false;
    FVector End = Location;
    End.Z += 0.1f;
    FHitResult Hit{};
    const TArray<AActor*> Ignore{};
    // Check the entire capsule, including its head, independently of actor collision being disabled.
    const bool Blocked = UKismetSystemLibrary::CapsuleTraceSingleByProfile(Player, Location, End,
        Radius - 0.5f, Height - 0.5f, Capsule->GetCollisionProfileName(), false, Ignore,
        EDrawDebugTrace::None, &Hit, true, FLinearColor{}, FLinearColor{}, 0.0f);
    return !Blocked && !Hit.bStartPenetrating;
}

inline bool TryPlace(ABP_PlayerCharacter_C* Player, APlayerController* Controller, const Anchor& Point) {
    if (!ValidAnchor(Point) || !FloorAt(Player, Point.Transform.Translation, 40.0))
        return false;
    FVector Destination = Point.Transform.Translation;
    // Anchors are actor/capsule centres, not navmesh foot positions. Small clearance only.
    Destination.Z += 4.0f;
    if (!FloorAt(Player, Destination, 45.0) || !CapsuleFits(Player, Destination))
        return false;
    if (!Player->K2_TeleportTo(Destination, Point.Rotation))
        return false;
    const FVector Actual = Player->K2_GetActorLocation();
    if (!FloorAt(Player, Actual, 45.0) || !CapsuleFits(Player, Actual))
        return false;

    auto* Movement = Player->CharacterMovement;
    Player->SetActorEnableCollision(true);
    Player->CapsuleComponent->SetCollisionEnabled(Point.Collision);
    Movement->StopMovementImmediately();
    Movement->ClearAccumulatedForces();
    Movement->SetMovementMode(EMovementMode::MOVE_Walking, 0);
    Movement->bJustTeleported = true;
    Movement->bForceNextFloorCheck = true;
    Player->TeleportDestination = Actual;
    Player->DestinationVelocity = FVector{};
    Player->LastValidPlayerTransform = Player->GetTransform();
    Player->InFallRecovery = false;
    Player->IsFalling = false;
    // Use the game's reliable owning-client recovery event, not client-side position spoofing.
    Controller->ClientSetLocation(Actual, Point.Rotation);
    Player->OnRecoverFromEdgeFall();
    Player->ForceNetUpdate();
    return true;
}

inline void Tick(UNetDriver* Driver, float Seconds) {
    UseWorld(UWorld::GetWorld());
    Clock += SafeDelta(Seconds);
    PollSeconds += SafeDelta(Seconds);
    if (!CurrentWorld || !Driver) {
        Players.clear();
        Starts.clear();
        return;
    }
    // Ten checks/second; do not run extra floor sweeps at the uncapped engine tick rate.
    if (PollSeconds < 0.1)
        return;
    const double Step = PollSeconds;
    PollSeconds = 0.0;
    std::set<UNetConnection*> Active;
    for (UNetConnection* Connection : Driver->ClientConnections) {
        if (!Connection || !Connection->PlayerController)
            continue;
        auto* Controller = Connection->PlayerController;
        auto* Pawn = Controller->Pawn;
        if (!Pawn || !Pawn->IsA(ABP_PlayerCharacter_C::StaticClass()) || Pawn->IsActorBeingDestroyed()
            || ObjectWorld(Pawn) != CurrentWorld)
            continue;
        auto* Player = static_cast<ABP_PlayerCharacter_C*>(Pawn);
        if (!Player->HasAuthority() || !Player->CharacterMovement || !Player->CapsuleComponent)
            continue;
        Active.insert(Connection);
        auto& State = Players[Connection];
        if (State.Pawn != Player || State.Controller != Controller) {
            State = PlayerState{};
            State.Pawn = Player;
            State.Controller = Controller;
        }
        // Consume the native spawn once; records for failed joins expire without dereferencing their keys.
        auto Start = Starts.find(static_cast<AArchonPlayerState*>(Controller->PlayerState));
        if (Start != Starts.end() && (!Start->second.Controller || Start->second.Controller == Controller)) {
            State.Spawn = Start->second.Position;
            State.HasSpawn = true;
            Starts.erase(Start);
        }
        auto* Movement = Player->CharacterMovement;
        const FVector Position = Player->K2_GetActorLocation();
        const bool Eligible = !Player->IsDying && !Player->Bleed_Out_State && FinitePosition(Position);
        const bool Walking = Movement->MovementMode == EMovementMode::MOVE_Walking
            || Movement->MovementMode == EMovementMode::MOVE_NavWalking;
        const auto& Floor = Movement->CurrentFloor;
        const bool Grounded = Walking && Player->GetActorEnableCollision()
            && ValidFloor(Floor.bBlockingHit, Floor.bWalkableFloor, Floor.HitResult.bStartPenetrating,
                Floor.bLineTrace ? Floor.LineDist : Floor.FloorDist, 12.0)
            && FloorAt(Player, Position, 12.0);
        Sample Input;
        Input.Eligible = Eligible;
        Input.Grounded = Grounded;
        Input.Recovering = Player->InFallRecovery;
        Input.Falling = Movement->MovementMode == EMovementMode::MOVE_Falling;
        Input.VerticalSpeed = Movement->Velocity.Z;
        if (State.Count > 0)
            Input.DropFromSafe = State.Ground[0].Transform.Translation.Z - Position.Z;
        else if (State.HasSpawn)
            Input.DropFromSafe = State.Spawn.Transform.Translation.Z - Position.Z;
        if (auto* Settings = CurrentWorld->K2_GetWorldSettings())
            Input.BelowKillZ = std::isfinite(Settings->KillZ) && Position.Z < Settings->KillZ;
        const Decision Action = State.Watchdog.Advance(Step, Input, State.Count > 0 || State.HasSpawn);
        if (!Eligible) {
            // A later resurrection must earn a fresh ground position; never revive or heal here.
            State.Count = 0;
            continue;
        }
        if (Action.CaptureGround && FloorAt(Player, Position, 12.0) && CapsuleFits(Player, Position)) {
            const Anchor Point{Player->GetTransform(), Player->K2_GetActorRotation(),
                Player->CapsuleComponent->GetCollisionEnabled()};
            if (ValidAnchor(Point)) {
                for (size_t i = State.Ground.size() - 1; i > 0; --i)
                    State.Ground[i] = State.Ground[i - 1];
                State.Ground[0] = Point;
                State.Count = (std::min)(State.Count + 1, State.Ground.size());
                Player->LastValidPlayerTransform = Point.Transform;
            }
        }
        if (!Action.Recover)
            continue;
        bool Restored = false;
        for (size_t i = 0; i < State.Count && !Restored; ++i)
            Restored = TryPlace(Player, Controller, State.Ground[i]);
        if (!Restored && State.HasSpawn)
            Restored = TryPlace(Player, Controller, State.Spawn);
        if (Restored)
            State.Watchdog.Teleported();
        // No account identifiers, keys, health changes, travel, or hunt restarts.
        std::cout << "[fall-recovery] " << (Restored ? "restored" : "no safe destination")
                  << " port-player=" << Controller->GetName() << " grounded-history=" << State.Count
                  << " recovery=" << Input.Recovering << " long-fall-drop=" << Input.DropFromSafe << std::endl;
    }
    for (auto It = Players.begin(); It != Players.end();)
        if (!Active.contains(It->first)) It = Players.erase(It); else ++It;
    for (auto It = Starts.begin(); It != Starts.end();)
        if (It->second.Expires <= Clock) It = Starts.erase(It); else ++It;
}

} // namespace FallRecovery
