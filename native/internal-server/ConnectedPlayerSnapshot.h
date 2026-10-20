#pragma once

#include <cstdint>
#include <mutex>
#include <string>
#include <string_view>
#include <unordered_set>
#include <utility>
#include <vector>

// This header has no engine dependencies. Only a complete, bounded snapshot may
// be sent: an omitted header must never be interpreted as an empty server.
namespace ConnectedPlayerSnapshot {
    inline constexpr int MaxConnections = 128;
    inline constexpr std::size_t MaxPlayerIdLength = 128;
    inline constexpr std::size_t MaxHeaderLength = 8192;
    inline constexpr std::uint64_t MaxAgeMilliseconds = 2000;

    inline std::wstring SessionIdFromReadyFile(std::wstring_view Path) {
        const auto Separator = Path.find_last_of(L"/\\");
        const auto Name = Separator == std::wstring_view::npos ? Path : Path.substr(Separator + 1);
        if (Name.size() != 42 || Name.substr(36) != L".ready") return {};
        std::wstring Id(Name.substr(0, 36));
        for (std::size_t i = 0; i < Id.size(); ++i) {
            if (i == 8 || i == 13 || i == 18 || i == 23) {
                if (Id[i] != L'-') return {};
            }
            else if (Id[i] >= L'A' && Id[i] <= L'F') Id[i] += L'a' - L'A';
            else if (!(Id[i] >= L'a' && Id[i] <= L'f') && !(Id[i] >= L'0' && Id[i] <= L'9')) return {};
        }
        return Id;
    }

    class Builder {
        bool Complete = true;
        int Connections = 0;
        std::vector<std::wstring> PlayerIds;
        std::unordered_set<std::wstring> Seen;

    public:
        void Invalidate() { Complete = false; }

        void AddConnection(std::uint32_t State, std::wstring_view Id = {}) {
            if (++Connections > MaxConnections) { Complete = false; return; }
            // USOCK_Closed is the only state that proves this entry no longer
            // needs an identity. Invalid/unknown states are conservatively kept.
            if (State == 1) return;
            if ((State != 2 && State != 3) || Id.empty() || Id.size() > MaxPlayerIdLength) {
                Complete = false;
                return;
            }
            for (const wchar_t Character : Id) {
                if (Character < 0x21 || Character > 0x7e) {
                    Complete = false;
                    return;
                }
            }
            if (Seen.emplace(Id).second) PlayerIds.emplace_back(Id);
        }

        std::wstring Header() const {
            if (!Complete) return {};
            std::wstring Json = L"[";
            for (const auto& Id : PlayerIds) {
                if (Json.size() > 1) Json += L',';
                Json += L'"';
                for (const wchar_t Character : Id) {
                    if (Character == L'"' || Character == L'\\') Json += L'\\';
                    Json += Character;
                }
                Json += L'"';
                if (Json.size() + 1 > MaxHeaderLength) return {};
            }
            Json += L']';
            return Json;
        }
    };

    class Cache {
        std::mutex Mutex;
        std::wstring Value;
        std::uint64_t CapturedAt = 0;

    public:
        void Publish(std::wstring Header, std::uint64_t Now) {
            std::lock_guard<std::mutex> Lock(Mutex);
            Value = std::move(Header);
            CapturedAt = Now;
        }

        std::wstring Read(std::uint64_t Now) {
            std::lock_guard<std::mutex> Lock(Mutex);
            if (Now < CapturedAt || Now - CapturedAt > MaxAgeMilliseconds) return {};
            return Value;
        }
    };
}
