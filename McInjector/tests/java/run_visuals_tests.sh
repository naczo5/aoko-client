#!/usr/bin/env bash
# McInjector/tests/java/run_visuals_tests.sh
#
# Compiles the visuals helper (com.aoko.visuals) as Java 8 against LWJGL 2 and runs the
# dependency-free core test over the shipped assets. LWJGL is only needed to compile
# LegacyGl; set LWJGL_JAR to a lwjgl-2.9.x jar (Maven: org.lwjgl.lwjgl:lwjgl).
#
# Usage: LWJGL_JAR=/path/lwjgl-2.9.3.jar ./run_visuals_tests.sh
set -euo pipefail

cd "$(dirname "$0")/../.."

JAVAC="${JAVAC:-javac}"
JAVA="${JAVA:-java}"
: "${LWJGL_JAR:?set LWJGL_JAR to a lwjgl 2.9.x jar}"
OUT="$(mktemp -d)"
cleanup() { rm -rf "$OUT"; }
trap cleanup EXIT

"$JAVAC" --release 8 -Xlint:all,-options -cp "$LWJGL_JAR" -d "$OUT" src/main/java/com/aoko/visuals/*.java tests/java/VisualsCoreTest.java
"$JAVA" -ea -cp "$OUT" com.aoko.visuals.VisualsCoreTest ../Aoko/Assets/Visuals
