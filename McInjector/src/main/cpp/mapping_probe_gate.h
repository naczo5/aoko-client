#pragma once

#include <atomic>

// Small state machine used by lazy JNI mapping probes.  A failed optional
// lookup is still a useful result for the current mapping generation: retrying
// it on every scan only recreates the same JNI exception and log churn.  The
// owning bridge resets the generation when mappings are invalidated or
// explicitly reloaded.
namespace lc {

class MappingProbeGate {
public:
    MappingProbeGate() : generation_(0), attempted_(false), lock_(ATOMIC_FLAG_INIT) {}

    // Returns true exactly once for each non-zero generation.  A zero
    // generation is treated as an uninitialized token and is intentionally
    // allowed once so standalone callers can use a simple counter.
    bool Begin(unsigned long long generation) {
        LockGuard guard(*this);
        if (generation_ != generation) {
            generation_ = generation;
            attempted_ = false;
        }
        if (attempted_) return false;
        attempted_ = true;
        return true;
    }

    void Reset() {
        LockGuard guard(*this);
        generation_ = 0;
        attempted_ = false;
    }

    unsigned long long Generation() const {
        LockGuard guard(*this);
        return generation_;
    }
    bool Attempted() const {
        LockGuard guard(*this);
        return attempted_;
    }

private:
    class LockGuard {
    public:
        explicit LockGuard(const MappingProbeGate& owner) : owner_(owner) {
            while (owner_.lock_.test_and_set(std::memory_order_acquire)) {}
        }
        ~LockGuard() { owner_.lock_.clear(std::memory_order_release); }
    private:
        const MappingProbeGate& owner_;
    };

    unsigned long long generation_;
    bool attempted_;
    mutable std::atomic_flag lock_;
};

} // namespace lc
