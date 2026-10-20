#include "../ConnectedPlayerSnapshot.h"

#include <atomic>
#include <cassert>
#include <iostream>
#include <thread>

using namespace ConnectedPlayerSnapshot;

static void SessionIdentity() {
    const std::wstring Id = L"d7122b52-7217-4e45-8e66-dac6350a5cf1";
    assert(SessionIdFromReadyFile(L"/tmp/ready/" + Id + L".ready") == Id);
    assert(SessionIdFromReadyFile(L"C:\\games\\ready\\D7122B52-7217-4E45-8E66-DAC6350A5CF1.ready") == Id);
    assert(SessionIdFromReadyFile(Id + L".ready") == Id);
    assert(SessionIdFromReadyFile(L"").empty());
    assert(SessionIdFromReadyFile(L"/tmp/" + Id + L".ready/another-file").empty());
    assert(SessionIdFromReadyFile(L"/tmp/" + Id + L".ready.tmp").empty());
    assert(SessionIdFromReadyFile(L"/tmp/not-a-session.ready").empty());
    assert(SessionIdFromReadyFile(L"z7122b52-7217-4e45-8e66-dac6350a5cf1.ready").empty());
    assert(SessionIdFromReadyFile(L"d7122b52_7217-4e45-8e66-dac6350a5cf1.ready").empty());
}

static void CompleteAndIncompleteConnections() {
    Builder Empty;
    assert(Empty.Header() == L"[]");
    Empty.AddConnection(1);
    assert(Empty.Header() == L"[]");

    Builder Connected;
    Connected.AddConnection(3, L"GWOG-UID-1");
    Connected.AddConnection(2, L"GWOG-UID-2");
    Connected.AddConnection(3, L"GWOG-UID-1");
    Connected.AddConnection(1);
    assert(Connected.Header() == L"[\"GWOG-UID-1\",\"GWOG-UID-2\"]");

    for (const auto State : {0U, 2U, 3U, 4U, 0xffffffffU}) {
        Builder Unknown;
        Unknown.AddConnection(3, L"connected");
        Unknown.AddConnection(State); // Missing controller, player state or ID.
        assert(Unknown.Header().empty());
    }
    for (const auto State : {0U, 4U}) {
        Builder UnknownState;
        UnknownState.AddConnection(State, L"identified-but-state-unknown");
        assert(UnknownState.Header().empty());
    }
    Builder MissingChild;
    MissingChild.AddConnection(3, L"parent");
    MissingChild.Invalidate();
    assert(MissingChild.Header().empty());
}

static void JsonAndLimits() {
    Builder Escaped;
    Escaped.AddConnection(3, L"quote\"back\\slash");
    assert(Escaped.Header() == L"[\"quote\\\"back\\\\slash\"]");

    for (const auto& Id : std::vector<std::wstring>{L"with space", L"with\r\ninjection", L"with\ttab",
             L"nonascii\u00e4", std::wstring(L"nul\0inside", 10), std::wstring(129, L'x')}) {
        Builder Invalid;
        Invalid.AddConnection(3, Id);
        assert(Invalid.Header().empty());
    }
    Builder LongestId;
    LongestId.AddConnection(3, std::wstring(128, L'x'));
    assert(!LongestId.Header().empty());

    Builder TooMany;
    for (int i = 0; i < MaxConnections; ++i) TooMany.AddConnection(3, L"player-" + std::to_wstring(i));
    assert(!TooMany.Header().empty());
    TooMany.AddConnection(3, L"one-too-many");
    assert(TooMany.Header().empty());

    Builder TooLarge;
    for (int i = 0; i < 62; ++i) {
        auto Id = std::to_wstring(i);
        Id.resize(MaxPlayerIdLength, L'x');
        TooLarge.AddConnection(3, Id);
    }
    assert(TooLarge.Header().size() <= MaxHeaderLength && !TooLarge.Header().empty());
    TooLarge.AddConnection(3, std::wstring(MaxPlayerIdLength, L'y'));
    assert(TooLarge.Header().empty());
}

static void FreshnessAndPublication() {
    Cache Snapshot;
    assert(Snapshot.Read(1000).empty());
    Snapshot.Publish(L"[]", 1000);
    assert(Snapshot.Read(1000) == L"[]");
    assert(Snapshot.Read(1000 + MaxAgeMilliseconds) == L"[]");
    assert(Snapshot.Read(1001 + MaxAgeMilliseconds).empty());
    assert(Snapshot.Read(999).empty());
    Snapshot.Publish(L"[\"connected\"]", 4000);
    assert(Snapshot.Read(4001) == L"[\"connected\"]");
    Snapshot.Publish({}, 4002); // A new unidentified pending connection invalidates immediately.
    assert(Snapshot.Read(4002).empty());

    std::atomic<bool> Start = false;
    std::atomic<bool> Done = false;
    std::vector<std::thread> Readers;
    for (int i = 0; i < 3; ++i) Readers.emplace_back([&] {
        while (!Start.load()) std::this_thread::yield();
        do {
            const auto Header = Snapshot.Read(5000);
            assert(Header.empty() || Header == L"[]" || Header == L"[\"connected\"]");
        } while (!Done.load());
    });
    Start = true;
    for (int i = 0; i < 10000; ++i) Snapshot.Publish(i % 2 == 0 ? L"[]" : L"[\"connected\"]", 5000);
    Done = true;
    for (auto& Reader : Readers) Reader.join();
}

int main() {
    SessionIdentity();
    CompleteAndIncompleteConnections();
    JsonAndLimits();
    FreshnessAndPublication();
    std::cout << "connected-player-snapshot tests passed\n";
}
