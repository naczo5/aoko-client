#!/usr/bin/env bash
# McInjector/tests/java/run_tests.sh
#
# Compiles com.aoko.helper.SessionSwitcher with the same flags the embed generator
# uses and runs the dependency-free structural test. No Minecraft jar is needed:
# the legacy 1.8.9, Forge SRG, 1.21 and 26.x session shapes are faked in the test.
#
# Usage: ./run_tests.sh
set -euo pipefail

cd "$(dirname "$0")/../.."

JAVAC="${JAVAC:-javac}"
JAVA="${JAVA:-java}"
OUT_DIR="$(mktemp -d)"
cleanup() { rm -rf "$OUT_DIR"; }
trap cleanup EXIT

"$JAVAC" --release 8 -Xlint:all,-options \
    -d "$OUT_DIR" \
    src/main/java/com/aoko/helper/SessionSwitcher.java \
    tests/java/SessionSwitcherStructuralTest.java

"$JAVA" -ea -cp "$OUT_DIR" com.aoko.helper.SessionSwitcherStructuralTest
