#pragma once
#include <unordered_map>

// A game-thread, tick-local index. Store only actor keys and array slots: a channel
// returned by Get is live, whereas a cached channel pointer may already be freed.
//
// An absent actor is a safe O(1) miss only when every new actor/channel association
// calls ActorAssigned. Removal, reordering and clearing an association need no
// notification: they cannot introduce an actor that is absent from this index.
// With no assignment observer, preserve the conservative scan on every miss.
template<class Actor>
struct ChannelIndex {
    std::unordered_map<Actor*, int> Slots;
    std::unordered_map<Actor*, unsigned int> AssignmentsInProgress;
    bool AssignmentsObserved = false;

    void ActorAssigned(Actor* Value) {
        if (!Value) return;
        ++AssignmentsInProgress[Value];
        Slots.try_emplace(Value, -1);
    }

    template<class At, class ActorOf>
    void AssignmentCompleted(Actor* Value, int Count, At Get, ActorOf GetActor) {
        if (!Value) return;
        Slots.try_emplace(Value, -1);
        const auto Pending = AssignmentsInProgress.find(Value);
        if (Pending != AssignmentsInProgress.end() && --Pending->second == 0) AssignmentsInProgress.erase(Pending);
        // Appends are the ordinary creation path. Read through the current
        // array, never through the pointer supplied to the engine setter.
        if (Count > 0 && GetActor(Get(Count - 1)) == Value) Slots[Value] = Count - 1;
    }
};

template<class Actor, class At, class ActorOf>
void RebuildChannelIndex(ChannelIndex<Actor>& Index, int Count, At Get, ActorOf GetActor) {
    Index.Slots.clear();
    Index.Slots.reserve(Count);
    for (int i = 0; i < Count; ++i)
        if (auto* Value = GetActor(Get(i))) Index.Slots.try_emplace(Value, i);
    // Reentrant lookups may run both before and after the setter writes Actor.
    // Preserve in-flight membership until its matching completion notification.
    for (const auto& Pending : Index.AssignmentsInProgress) Index.Slots.try_emplace(Pending.first, -1);
}

template<class Actor, class At, class ActorOf>
auto IndexChannels(int Count, At Get, ActorOf GetActor, bool AssignmentsObserved = false) {
    ChannelIndex<Actor> Index;
    Index.AssignmentsObserved = AssignmentsObserved;
    RebuildChannelIndex(Index, Count, Get, GetActor);
    return Index;
}

template<class Actor, class At, class ActorOf>
auto FindChannel(Actor* ActorToFind, ChannelIndex<Actor>& Index, int Count, At Get, ActorOf GetActor) -> decltype(Get(0)) {
    if (!ActorToFind) return nullptr;

    auto Found = Index.Slots.find(ActorToFind);
    if (Found == Index.Slots.end() && Index.AssignmentsObserved) return nullptr;

    if (Found != Index.Slots.end() && Found->second >= 0 && Found->second < Count) {
        auto* Channel = Get(Found->second);
        if (GetActor(Channel) == ActorToFind) return Channel;
    }

    // SetChannelActor normally assigns a just-appended channel. Resolve its
    // notified but not yet indexed association without rescanning the prefix.
    if (Found != Index.Slots.end() && Count > 0) {
        auto* Channel = Get(Count - 1);
        if (GetActor(Channel) == ActorToFind) {
            Found->second = Count - 1;
            return Channel;
        }
    }

    // A stale positive slot (or an unobserved miss) requires one live rebuild.
    // Refresh every slot so a bulk removal/reorder does not cause a scan for
    // each actor after it. No engine calls may mutate the array during a lookup.
    RebuildChannelIndex(Index, Count, Get, GetActor);
    Found = Index.Slots.find(ActorToFind);
    return Found != Index.Slots.end() && Found->second >= 0 && Found->second < Count
        ? Get(Found->second) : nullptr;
}
