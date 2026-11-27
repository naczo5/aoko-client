#!/usr/bin/env bash
# McInjector/tests/java/run_real_hook_checks.sh
#
# Applies the production hook table (visuals_hooks.h) to the real game classes, checks that
# every spec matched exactly one method, and has the JVM verify the rewritten classes.
#
# Usage: run_real_hook_checks.sh <legacy|modern> <client.jar> <classpath-of-libs> [java]
#   legacy: a 1.8.9 jar renamed to MCP or SRG names; modern: an unobfuscated 26.x client jar.
set -euo pipefail
cd "$(dirname "$0")/.."

GEN="$1"; JAR="$2"; LIBS="$3"; JAVA="${4:-java}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

g++ -std=c++11 -Wall -Wextra -O1 -o "$WORK/check" visuals_hooks_check.cpp
mkdir -p "$WORK/classes"
(cd "$WORK/classes" && unzip -q -o "$JAR" 'net/*' 'com/*' 2>/dev/null || true)
"$WORK/check" "$GEN" "$WORK/classes" "$WORK/out" | tee "$WORK/check.txt"
javac --release 8 -d "$WORK/test" java/HookVerifyTest.java
# Use the extracted classes, not the jar: mixing rewritten (unsigned) classes with a signed jar
# in one package is a SecurityException.
mkdir -p "$WORK/run"
(cd "$WORK/run" && "$JAVA" -Xverify:all -cp "$WORK/test" HookVerifyTest "$WORK/out" "$WORK/classes:$LIBS")

ASSETS="$(cd ../.. && pwd)/Aoko/Assets/Visuals"
mkdir -p "$WORK/helper" "$WORK/run"

# Helper contract: every guard/value method the native table calls exists in Hooks.
: "${LWJGL_JAR:?set LWJGL_JAR (compile-time only) to compile the helper}"
javac --release 8 -Xlint:all,-options -cp "$LWJGL_JAR" -d "$WORK/helper" ../src/main/java/com/aoko/visuals/*.java
javac --release 8 -d "$WORK/test" java/HooksContractTest.java
"$JAVA" -cp "$WORK/test:$WORK/helper" HooksContractTest "$WORK/check.txt"

# End to end: rewritten classes plus the real helper, hooked methods called. The end to end
# test reads fields by MCP name, so it is skipped for SRG-named jars (SKIP_E2E=1).
if [ "${SKIP_E2E:-0}" = "1" ]; then echo "end to end check skipped"; exit 0; fi
javac --release 8 -d "$WORK/test" java/HookE2ETest.java 2>&1 | grep -v "^warning\|^[0-9] warning" || true
# Run from a scratch directory: the game's logger writes logs/ into the working directory.
(cd "$WORK/run" && "$JAVA" -cp "$WORK/test" HookE2ETest "$GEN" "$WORK/out" "$WORK/helper" "$WORK/classes:$LIBS:$LWJGL_JAR" "$ASSETS")
