#pragma once

#include <atomic>
#include <cstddef>

// Shadow-mode ownership tracker for exclusive gameplay resources.
//
// The existing modules already enforce these locks with ad-hoc predicates. This
// arbiter does not gate anything yet: it records which module believes it owns a
// resource and counts cross-module conflicts so a later enforcement pass can be
// validated against real sessions first.
namespace lc {

enum class ExclusiveResource : int {
    Hotbar = 0,
    UseItem = 1,
    Attack = 2,
    WindowClick = 3,
    Look = 4,
    Count = 5
};

enum class ExclusiveOwner : int {
    None = 0,
    AutoRod,
    ThrowPot,
    AutoHeal,
    AutoTool,
    AutoTotem,
    KillAuraBlock,
    FastPlace,
    ChestStealer
};

inline const char* ExclusiveResourceName(ExclusiveResource resource) {
    switch (resource) {
    case ExclusiveResource::Hotbar: return "hotbar";
    case ExclusiveResource::UseItem: return "use-item";
    case ExclusiveResource::Attack: return "attack";
    case ExclusiveResource::WindowClick: return "window-click";
    case ExclusiveResource::Look: return "look";
    default: return "unknown";
    }
}

inline const char* ExclusiveOwnerName(ExclusiveOwner owner) {
    switch (owner) {
    case ExclusiveOwner::AutoRod: return "AutoRod";
    case ExclusiveOwner::ThrowPot: return "ThrowPot";
    case ExclusiveOwner::AutoHeal: return "AutoHeal";
    case ExclusiveOwner::AutoTool: return "AutoTool";
    case ExclusiveOwner::AutoTotem: return "AutoTotem";
    case ExclusiveOwner::KillAuraBlock: return "KillAuraBlock";
    case ExclusiveOwner::FastPlace: return "FastPlace";
    case ExclusiveOwner::ChestStealer: return "ChestStealer";
    default: return "None";
    }
}

class ExclusiveResourceArbiter {
public:
    struct Attempt {
        bool conflict;
        ExclusiveOwner previous;
    };

    ExclusiveResourceArbiter() {
        for (int i = 0; i < static_cast<int>(ExclusiveResource::Count); ++i) {
            owners_[i].store(static_cast<int>(ExclusiveOwner::None),
                             std::memory_order_relaxed);
            conflicts_[i].store(0, std::memory_order_relaxed);
        }
    }

    // Records an ownership claim. If another owner already holds the resource,
    // the claim is counted as a conflict and the existing owner is preserved.
    Attempt ObserveAttempt(ExclusiveResource resource, ExclusiveOwner owner) {
        Attempt result = { false, ExclusiveOwner::None };
        const int index = IndexOf(resource);
        if (index < 0 || owner == ExclusiveOwner::None) return result;

        int expected = static_cast<int>(ExclusiveOwner::None);
        const int desired = static_cast<int>(owner);
        if (owners_[index].compare_exchange_strong(expected, desired,
                                                   std::memory_order_acq_rel)) {
            return result;
        }

        result.conflict = expected != desired;
        result.previous = static_cast<ExclusiveOwner>(expected);
        if (result.conflict) {
            conflicts_[index].fetch_add(1, std::memory_order_relaxed);
        }
        return result;
    }

    void Release(ExclusiveResource resource, ExclusiveOwner owner) {
        const int index = IndexOf(resource);
        if (index < 0 || owner == ExclusiveOwner::None) return;
        int expected = static_cast<int>(owner);
        owners_[index].compare_exchange_strong(expected,
                                               static_cast<int>(ExclusiveOwner::None),
                                               std::memory_order_acq_rel);
    }

    ExclusiveOwner OwnerOf(ExclusiveResource resource) const {
        const int index = IndexOf(resource);
        if (index < 0) return ExclusiveOwner::None;
        return static_cast<ExclusiveOwner>(owners_[index].load(std::memory_order_acquire));
    }

    unsigned long long ConflictCount(ExclusiveResource resource) const {
        const int index = IndexOf(resource);
        return index < 0 ? 0 : conflicts_[index].load(std::memory_order_relaxed);
    }

    void Reset() {
        for (int i = 0; i < static_cast<int>(ExclusiveResource::Count); ++i) {
            owners_[i].store(static_cast<int>(ExclusiveOwner::None),
                             std::memory_order_release);
            conflicts_[i].store(0, std::memory_order_relaxed);
        }
    }

private:
    static int IndexOf(ExclusiveResource resource) {
        const int index = static_cast<int>(resource);
        return index >= 0 && index < static_cast<int>(ExclusiveResource::Count)
            ? index : -1;
    }

    std::atomic<int> owners_[static_cast<int>(ExclusiveResource::Count)];
    std::atomic<unsigned long long> conflicts_[static_cast<int>(ExclusiveResource::Count)];
};

} // namespace lc
