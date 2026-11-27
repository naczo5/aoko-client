#!/usr/bin/env bash
# McInjector/tests/java/run_legacy_harness.sh
#
# Runs the LegacyPlatform against the real Minecraft 1.8.9 client classes renamed to MCP
# names (SpecialSource over the vanilla jar with the MCP stable_22 and SRG tables).
# Set: MC_JAR (the MCP-named jar) MC_LIBS (colon separated libraries) LWJGL_JAR JAVA (any JDK 8+).
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${MC_JAR:?}" "${MC_LIBS:?}" "${LWJGL_JAR:?}"
JAVA="${JAVA:-java}"
ASSETS="$(cd .. && pwd)/Aoko/Assets/Visuals"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT
javac --release 8 -Xlint:all,-options -cp "$LWJGL_JAR" -d "$OUT" src/main/java/com/aoko/visuals/*.java tests/java/harness/LegacyHarness.java
mkdir -p "$OUT/run"
(cd "$OUT/run" && "$JAVA" -cp "$OUT:$MC_JAR:$MC_LIBS:$LWJGL_JAR" com.aoko.visuals.LegacyHarness "$ASSETS")
