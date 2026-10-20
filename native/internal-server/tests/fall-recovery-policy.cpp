#include "../FallRecoveryPolicy.h"
#include <cstdlib>
#include <iostream>
#include <limits>

using namespace FallRecovery;

static int Checks = 0;
static void Check(bool Condition, const char* Message) {
    ++Checks;
    if (!Condition) {
        std::cerr << "FAIL: " << Message << '\n';
        std::exit(1);
    }
}

static int Advance(Policy& State, const Sample& Input, int Frames, bool HasDestination = true) {
    int Recoveries = 0;
    for (int i = 0; i < Frames; ++i)
        Recoveries += State.Advance(0.125, Input, HasDestination).Recover ? 1 : 0;
    return Recoveries;
}

int main() {
    const double Nan = std::numeric_limits<double>::quiet_NaN();
    const double Inf = std::numeric_limits<double>::infinity();
    Check(SafeDelta(Nan) == 0.0, "NaN delta ignored");
    Check(SafeDelta(Inf) == 0.0, "infinite delta ignored");
    Check(SafeDelta(-1.0) == 0.0, "negative delta ignored");
    Check(SafeDelta(30.0) == 0.25, "debugger/lag spike clamped");
    Check(ValidFloor(true, true, false, 2.0, 12.0), "real walkable floor accepted");
    Check(!ValidFloor(false, true, false, 2.0, 12.0), "missing floor rejected");
    Check(!ValidFloor(true, false, false, 2.0, 12.0), "wall/nonwalkable floor rejected");
    Check(!ValidFloor(true, true, true, 2.0, 12.0), "capsule embedded in floor rejected");
    Check(!ValidFloor(true, true, false, 200.0, 12.0), "distant floor rejected");
    Check(!ValidFloor(true, true, false, -5.0, 12.0), "floor penetration rejected");
    Check(!ValidFloor(true, true, false, Nan, 12.0), "NaN floor rejected");
    struct Vector { double X, Y, Z; };
    Check(FinitePosition(Vector{0, 0, 0}), "origin alone is not proof of an invalid point");
    Check(!FinitePosition(Vector{0, Inf, 0}), "nonfinite coordinate rejected");
    Check(!FinitePosition(Vector{Nan, 0, 0}), "NaN coordinate rejected");

    Sample Ground;
    Ground.Eligible = Ground.Grounded = true;
    {
        Policy State;
        for (int i = 0; i < 3; ++i)
            Check(!State.Advance(0.125, Ground, false).CaptureGround, "no early spawn snapshot");
        Check(State.Advance(0.125, Ground, false).CaptureGround, "stable ground earns first snapshot");
        Check(Advance(State, Ground, 200) == 0, "ordinary walking never rescues");
    }
    Sample Falling;
    Falling.Eligible = Falling.Falling = true;
    Falling.DropFromSafe = 5000.0;
    Falling.VerticalSpeed = -1000.0;
    {
        Policy State;
        Check(Advance(State, Falling, 79) == 0, "native fall/normal jump gets time to finish");
        Check(Advance(State, Falling, 1) == 1, "prolonged falling rescued even without Blueprint flag");
        Check(!State.Advance(0.125, Falling, true).CaptureGround, "falling position is never saved");
        Check(Advance(State, Falling, 23) == 1, "persistent fall retries after cooldown");
    }
    {
        Policy State;
        Sample Jump = Falling;
        Jump.DropFromSafe = 50.0;
        Check(Advance(State, Jump, 200) == 0, "long air time without big drop is not a rescue");
        Jump.DropFromSafe = 5000.0;
        Jump.VerticalSpeed = 2000.0;
        Check(Advance(State, Jump, 200) == 0, "ascending launch is not a rescue");
    }
    {
        Policy State;
        Sample Flying = Falling;
        Flying.Falling = false;
        Check(Advance(State, Flying, 200) == 0, "flight/custom traversal is left alone");
    }
    {
        Policy State;
        Check(Advance(State, Falling, 200, false) == 0, "no destination means no origin teleport");
    }
    Sample Recovery = Falling;
    Recovery.Recovering = true;
    {
        Policy State;
        Check(Advance(State, Recovery, 23) == 0, "native recovery receives three-second grace");
        Check(Advance(State, Recovery, 1) == 1, "stuck recovery is rescued");
        Check(Advance(State, Recovery, 23) == 0, "failed placement does not spam retries");
        Check(Advance(State, Recovery, 1) == 1, "high recovery flag is not restricted to one rising edge");
    }
    {
        Policy State;
        State.Advance(0.125, Recovery, true);
        Sample Flicker = Recovery;
        Flicker.Recovering = false;
        Flicker.Falling = false;
        Check(Advance(State, Flicker, 23) == 1, "flag/movement flicker cannot hide an unresolved loop");
    }
    {
        Policy State;
        Advance(State, Recovery, 8);
        Advance(State, Ground, 8);
        Check(Advance(State, Ground, 100) == 0, "successful native landing cancels rescue");
    }
    {
        Policy State;
        Advance(State, Recovery, 24);
        State.Teleported();
        for (int i = 0; i < 7; ++i)
            Check(!State.Advance(0.125, Ground, false).CaptureGround, "post-teleport settlement protected");
        Check(State.Advance(0.125, Ground, false).CaptureGround, "settled recovery can save again");
        Check(Advance(State, Ground, 100) == 0, "finished rescue does not loop");
    }
    {
        Policy State;
        Sample Below = Falling;
        Below.BelowKillZ = true;
        Check(State.Advance(0.125, Below, true).Recover, "below-world fall rescued promptly");
        State.Teleported();
        Check(!State.Advance(0.125, Below, true).Recover, "KillZ also respects retry cooldown");
    }
    {
        Policy State;
        Sample Dead = Recovery;
        Dead.Eligible = false;
        Check(Advance(State, Dead, 200) == 0, "no rescue of dead/ineligible players");
        Check(Advance(State, Recovery, 23) == 0, "ineligible state clears timers");
    }
    {
        Policy State;
        for (double Delta : {Nan, Inf, -1.0, 0.0})
            Check(!State.Advance(Delta, Recovery, true).Recover, "invalid time cannot trigger recovery");
        Check(!State.Advance(100.0, Recovery, true).Recover, "one huge frame cannot trigger recovery");
    }
    {
        Policy First, Second;
        Advance(First, Recovery, 24);
        Check(Advance(Second, Ground, 100) == 0, "players have independent recovery timers");
        First = Policy{};
        Check(Advance(First, Recovery, 23) == 0, "world/pawn reset clears recovery history");
    }
    std::cout << "fall recovery policy: " << Checks << " checks passed\n";
}
