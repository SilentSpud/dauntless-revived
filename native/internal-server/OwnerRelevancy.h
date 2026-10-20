#pragma once

// Match owner-only actors to this connection. Cap traversal for malformed/cyclic ownership.
template<class Actor, class OwnerOf>
bool IsConnectionOwner(Actor* Value, Actor* Controller, Actor* Pawn, OwnerOf Owner) {
    for (int Depth = 0; Value && Depth < 64; ++Depth) {
        if (Value == Controller || (Pawn && Value == Pawn)) return true;
        auto* Next = Owner(Value);
        if (Next == Value) break;
        Value = Next;
    }
    return false;
}
