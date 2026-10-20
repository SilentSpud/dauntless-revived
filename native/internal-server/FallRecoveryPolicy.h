#pragma once

#include <algorithm>
#include <cmath>

namespace FallRecovery {

// Engine-independent decisions; positions are Unreal units, times are seconds.
struct Sample {
    bool Eligible = false;
    bool Grounded = false;
    bool Recovering = false;
    bool Falling = false;
    bool BelowKillZ = false;
    double DropFromSafe = 0.0;
    double VerticalSpeed = 0.0;
};

struct Decision {
    bool CaptureGround = false;
    bool Recover = false;
};

inline double SafeDelta(double Seconds) {
    // A pause, debugger stop or corrupt delta must not trigger an instant rescue.
    return std::isfinite(Seconds) && Seconds > 0.0 ? (std::min)(Seconds, 0.25) : 0.0;
}

template <typename Vector>
bool FinitePosition(const Vector& Position) {
    return std::isfinite(Position.X) && std::isfinite(Position.Y) && std::isfinite(Position.Z);
}

inline bool ValidFloor(bool Blocking, bool Walkable, bool Penetrating, double Distance, double Maximum) {
    return Blocking && Walkable && !Penetrating && std::isfinite(Distance)
        && Distance >= -0.5 && Distance <= Maximum;
}

class Policy {
    bool RecoveryPending = false;
    double GroundSeconds = 0.0;
    double CaptureSeconds = 0.0;
    double RecoverySeconds = 0.0;
    double FallingSeconds = 0.0;
    double CooldownSeconds = 0.0;
    double SettleSeconds = 0.0;

public:
    Decision Advance(double Seconds, const Sample& Input, bool HasDestination) {
        const double Delta = SafeDelta(Seconds);
        Decision Result;
        if (!Input.Eligible) {
            *this = Policy{};
            return Result;
        }
        CooldownSeconds = (std::max)(0.0, CooldownSeconds - Delta);
        SettleSeconds = (std::max)(0.0, SettleSeconds - Delta);
        const bool StableGround = Input.Grounded && !Input.Recovering && !Input.Falling;
        if (StableGround) {
            GroundSeconds += Delta;
            // Require sustained real floor contact, not a single stale walking frame.
            if (GroundSeconds >= 0.5 && SettleSeconds <= 0.0) {
                CaptureSeconds += Delta;
                if (!HasDestination || CaptureSeconds >= 0.5) {
                    Result.CaptureGround = true;
                    CaptureSeconds = 0.0;
                }
                RecoveryPending = false;
                RecoverySeconds = 0.0;
                FallingSeconds = 0.0;
            }
        } else {
            GroundSeconds = 0.0;
            CaptureSeconds = 0.0;
        }
        // A recovery flag that stays true must retry; an edge-trigger is insufficient.
        RecoveryPending = RecoveryPending || Input.Recovering;
        RecoverySeconds = RecoveryPending ? RecoverySeconds + Delta : 0.0;
        FallingSeconds = Input.Falling ? FallingSeconds + Delta : 0.0;
        const bool LongFall = FallingSeconds >= 10.0 && Input.DropFromSafe >= 3000.0
            && Input.VerticalSpeed < -300.0;
        const bool BelowWorld = Input.BelowKillZ && Input.Falling && Input.VerticalSpeed < 0.0;
        if (HasDestination && CooldownSeconds <= 0.0
            && (RecoverySeconds >= 3.0 || LongFall || BelowWorld)) {
            Result.Recover = true;
            // Rate-limit failed placements as well as successful ones.
            CooldownSeconds = 3.0;
        }
        return Result;
    }

    void Teleported() {
        RecoveryPending = false;
        GroundSeconds = CaptureSeconds = RecoverySeconds = FallingSeconds = 0.0;
        CooldownSeconds = 3.0;
        SettleSeconds = 1.0;
    }
};

} // namespace FallRecovery
