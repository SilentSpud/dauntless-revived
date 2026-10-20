#include "../ChannelLookup.h"
#include <algorithm>
#include <cassert>
#include <iostream>
#include <memory>
#include <random>
#include <vector>

struct Actor {};
struct Channel { Actor* actor; };

struct Connection {
    std::vector<Actor> actors;
    std::vector<std::unique_ptr<Channel>> channels;
    ChannelIndex<Actor> index;
    long long examined = 0;

    explicit Connection(int actorCount, bool observed = true) : actors(actorCount) {
        index.AssignmentsObserved = observed;
    }
    Channel* at(int slot) { return channels[slot].get(); }
    Actor* actorOf(Channel* channel) {
        ++examined;
        return channel ? channel->actor : nullptr;
    }
    void rebuild() {
        RebuildChannelIndex(index, static_cast<int>(channels.size()),
            [this](int slot) { return at(slot); }, [this](Channel* channel) { return actorOf(channel); });
    }
    Channel* find(Actor* actor) {
        return FindChannel(actor, index, static_cast<int>(channels.size()),
            [this](int slot) { return at(slot); }, [this](Channel* channel) { return actorOf(channel); });
    }
    Channel* find(int actor) { return find(&actors[actor]); }
    void assigned(Actor* actor) {
        index.AssignmentCompleted(actor, static_cast<int>(channels.size()),
            [this](int slot) { return at(slot); }, [this](Channel* channel) { return actorOf(channel); });
    }
    Channel* append(int actor, bool notify = true) {
        if (notify) index.ActorAssigned(&actors[actor]);
        channels.push_back(std::make_unique<Channel>(Channel{&actors[actor]}));
        if (notify) assigned(&actors[actor]);
        return channels.back().get();
    }
    void assertMatchesLiveChannels() {
        for (auto& actor : actors) {
            Channel* expected = nullptr;
            for (auto& channel : channels)
                if (channel && channel->actor == &actor) { expected = channel.get(); break; }
            assert(find(&actor) == expected);
        }
    }
};

static void SteadyState() {
    Connection connection(1000);
    for (int i = 0; i < 1000; ++i) connection.append(i, false);
    connection.rebuild();
    connection.examined = 0;
    for (int i = 0; i < 1000; ++i) assert(connection.find(i) == connection.at(i));
    assert(connection.examined == 1000);
}

static void ColdJoins() {
    for (const int count : {1000, 2000, 4000, 8000}) {
        Connection connection(count * 2);
        for (int i = 0; i < count; ++i) {
            assert(connection.find(i) == nullptr);
            auto* created = connection.append(i);
            assert(connection.find(i) == created);
            assert(connection.find(count + i) == nullptr);
        }
        // Includes both the setter notification and validation of the live slot.
        // The old cold join alone inspected N*(N-1)/2 channels.
        assert(connection.examined == 2LL * count);
        for (int i = 0; i < count; ++i) assert(connection.find(i) == connection.at(i));
        assert(connection.examined == 3LL * count);
        std::cout << "cold join actors=" << count << " channel inspections=" << connection.examined << '\n';
    }
}

static void RemovalAndReordering() {
    Connection connection(1000);
    for (int i = 0; i < 1000; ++i) connection.append(i, false);
    connection.rebuild();
    connection.examined = 0;
    connection.channels.erase(connection.channels.begin()); // Actually destroys the old channel.
    for (int i = 1; i < 1000; ++i) assert(connection.find(i) == connection.at(i - 1));
    assert(connection.find(0) == nullptr);
    assert(connection.examined < 3 * 1000); // Rebuild once, not once per shifted channel.

    std::reverse(connection.channels.begin(), connection.channels.end());
    connection.assertMatchesLiveChannels();
    connection.channels[2].reset();
    connection.assertMatchesLiveChannels();
    connection.channels.clear(); // Every previously indexed channel is freed.
    connection.assertMatchesLiveChannels();
}

static void InterleavedAssignmentsStayLinear() {
    constexpr int pairs = 1000;
    Connection connection(pairs * 2);
    for (int i = 0; i < pairs; ++i) {
        auto* first = connection.append(i);
        auto* indirect = connection.append(pairs + i);
        assert(connection.find(i) == first); // No longer the last array slot.
        assert(connection.find(pairs + i) == indirect);
    }
    assert(connection.examined == 4LL * pairs);
}

static void ReplacementsAndIndirectAdditions() {
    Connection connection(12);
    for (int i = 0; i < 4; ++i) connection.append(i, false);
    connection.rebuild();

    // Another engine path replaces a channel without changing the array size.
    // Ask for the new actor first: stale-positive validation cannot detect this.
    connection.index.ActorAssigned(&connection.actors[4]);
    connection.channels[1] = std::make_unique<Channel>(Channel{&connection.actors[4]});
    connection.assigned(&connection.actors[4]);
    assert(connection.find(4) == connection.at(1));
    assert(connection.find(1) == nullptr);

    // Reassigning an existing channel is also an association change.
    connection.index.ActorAssigned(&connection.actors[5]);
    connection.channels[2]->actor = &connection.actors[5];
    connection.assigned(&connection.actors[5]);
    assert(connection.find(5) == connection.at(2));
    assert(connection.find(2) == nullptr);

    // Channels created indirectly (for example during an RPC) use the same
    // assignment observer; none of them may be duplicated later in the pass.
    auto* indirect = connection.append(6);
    connection.append(7);
    assert(connection.find(6) == indirect);
    connection.channels.push_back(std::make_unique<Channel>(Channel{nullptr}));
    assert(connection.find(static_cast<Actor*>(nullptr)) == nullptr);
    connection.assertMatchesLiveChannels();

    // A lookup can rebuild before the original setter finishes assigning.
    connection.index.ActorAssigned(&connection.actors[8]);
    assert(connection.find(8) == nullptr);
    connection.channels[0]->actor = &connection.actors[8];
    assert(connection.find(8) == connection.at(0)); // Still inside the original setter.
    connection.assigned(&connection.actors[8]);
    assert(connection.find(8) == connection.at(0));
    assert(connection.find(0) == nullptr);
    assert(connection.index.AssignmentsInProgress.empty());

    // The inner completion must not erase an outer assignment's membership.
    connection.index.ActorAssigned(&connection.actors[9]);
    connection.index.ActorAssigned(&connection.actors[9]);
    connection.assigned(&connection.actors[9]);
    assert(connection.find(9) == nullptr); // Rebuild while the outer setter is still pending.
    connection.channels[0]->actor = &connection.actors[9];
    assert(connection.find(9) == connection.at(0));
    connection.assigned(&connection.actors[9]);
    assert(connection.index.AssignmentsInProgress.empty());
}

static void MissingObserverFallsBack() {
    Connection connection(5, false);
    connection.append(0, false);
    connection.rebuild();
    auto* added = connection.append(1, false);
    assert(connection.find(1) == added);
    connection.channels[0] = std::make_unique<Channel>(Channel{&connection.actors[2]});
    assert(connection.find(2) == connection.at(0));
    assert(connection.find(0) == nullptr);
    connection.assertMatchesLiveChannels();
}

static void MixedMutations() {
    Connection connection(128);
    std::mt19937 random(0xDA017);
    for (int iteration = 0; iteration < 500; ++iteration) {
        const auto operation = random() % 5;
        if (connection.channels.empty() || operation == 0) {
            std::vector<int> absent;
            for (int i = 0; i < static_cast<int>(connection.actors.size()); ++i)
                if (!connection.find(i)) absent.push_back(i);
            if (!absent.empty()) connection.append(absent[random() % absent.size()]);
        } else {
            const auto slot = random() % connection.channels.size();
            if (operation == 1) {
                connection.channels.erase(connection.channels.begin() + slot);
            } else if (operation == 2) {
                std::swap(connection.channels[slot], connection.channels.back());
                connection.channels.pop_back();
            } else if (operation == 3) {
                std::shuffle(connection.channels.begin(), connection.channels.end(), random);
            } else {
                // Replace a live channel with a new allocation for the same actor.
                auto* actor = connection.channels[slot]->actor;
                connection.index.ActorAssigned(actor);
                connection.channels[slot] = std::make_unique<Channel>(Channel{actor});
                connection.assigned(actor);
            }
        }
        connection.assertMatchesLiveChannels();
    }
}

int main() {
    SteadyState();
    ColdJoins();
    RemovalAndReordering();
    InterleavedAssignmentsStayLinear();
    ReplacementsAndIndirectAdditions();
    MissingObserverFallsBack();
    MixedMutations();
    std::cout << "channel lookup: cold joins, live mutations, freed channels and observer fallback passed\n";
}
