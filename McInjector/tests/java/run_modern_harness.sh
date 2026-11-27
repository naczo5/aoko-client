#!/usr/bin/env bash
# McInjector/tests/java/run_modern_harness.sh
#
# Runs the ModernPlatform against the real Minecraft 26.x client classes on a headless
# JVM (Java 25). Needs the unobfuscated client jar and its libraries; scripts/visuals
# does not download them. Set:
#   MC_JAR=/path/26.3-client.jar  MC_LIBS=/path/a.jar:/path/b.jar  JAVA25=/path/to/jdk25/bin/java
#   LWJGL_JAR=/path/lwjgl-2.9.3.jar (compile only)
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${MC_JAR:?}" "${MC_LIBS:?}" "${JAVA25:?}" "${LWJGL_JAR:?}"
ASSETS="$(cd .. && pwd)/Aoko/Assets/Visuals"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT
javac --release 8 -Xlint:all,-options -cp "$LWJGL_JAR" -d "$OUT" src/main/java/com/aoko/visuals/*.java tests/java/harness/ModernHarness.java
mkdir -p "$OUT/run"
(cd "$OUT/run" && "$JAVA25" -cp "$OUT:$MC_JAR:$MC_LIBS" com.aoko.visuals.ModernHarness "$ASSETS")
