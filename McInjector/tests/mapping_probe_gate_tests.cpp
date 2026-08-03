#include <iostream>

#include "../src/main/cpp/mapping_probe_gate.h"

static int g_failures = 0;

static void ExpectTrue(bool value, const char* message)
{
    if (!value) {
        std::cerr << "FAIL: " << message << std::endl;
        ++g_failures;
    }
}

static void ExpectFalse(bool value, const char* message)
{
    if (value) {
        std::cerr << "FAIL: " << message << std::endl;
        ++g_failures;
    }
}

int main()
{
    lc::MappingProbeGate gate;
    ExpectTrue(gate.Begin(1), "first probe is allowed");
    ExpectFalse(gate.Begin(1), "same generation is probed once");
    ExpectTrue(gate.Attempted(), "attempt is recorded");
    ExpectTrue(gate.Generation() == 1, "generation is retained");

    ExpectTrue(gate.Begin(2), "new mapping generation reopens probe");
    ExpectFalse(gate.Begin(2), "new generation still probes once");

    gate.Reset();
    ExpectTrue(gate.Begin(0), "reset allows an uninitialized generation");
    ExpectFalse(gate.Begin(0), "zero generation is still bounded");

    if (g_failures != 0) {
        std::cerr << "mapping_probe_gate_tests: " << g_failures
                  << " failure(s)" << std::endl;
        return 1;
    }

    std::cout << "mapping_probe_gate_tests: all passed" << std::endl;
    return 0;
}
