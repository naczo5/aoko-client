#!/usr/bin/env bash
# McInjector/tests/java/run_hook_tests.sh
#
# Builds the native method-hook rewriter CLI and the fixture classes, rewrites the
# fixtures (as Java 8 and as Java 6 class files), then loads original and rewritten
# classes in a JVM and compares behaviour. The JVM verifier checks every rewritten
# method while the class is defined. No Minecraft jar needed.
#
# Optional real-class check: set AOKO_MC_JARS to a colon-separated list of
# "label=path.jar" entries and see run_real_hook_checks.sh.
#
# Usage: ./run_hook_tests.sh
set -euo pipefail

cd "$(dirname "$0")/.."

JAVAC="${JAVAC:-javac}"
JAVA="${JAVA:-java}"
GXX="${GXX:-g++}"
WORK="$(mktemp -d)"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

"$GXX" -std=c++11 -Wall -Wextra -O1 -o "$WORK/mhook_cli" method_hook_rewriter_cli.cpp

mkdir -p "$WORK/build" "$WORK/test"
"$JAVAC" --release 8 -g -Xlint:all,-options -d "$WORK/build" java/hookfx/fx/Fx.java java/hookfx/fx/Hooks.java
"$JAVAC" -cp "$WORK/build" -d "$WORK/test" java/HookFixtureTest.java

SPECS=(
  "sInt:(IJL*;)I:gSInt:vSInt"
  "sFloat:(FD)F:gSFloat:vSFloat"
  "sLong:(JJ)J:gSLong:vSLong"
  "sDouble:(D)D:gSDouble:vSDouble"
  "iRef:(L*;I)L*;:gIRef:vIRef"
  "iStr:(L*;)L*;:gIStr:vIStr"
  "iVoid:(I)V:gIVoid"
  "iSync:(I)I:gISync:vISync"
  "iArr:(F)L*;:gIArr:vIArr"
  "loopAtStart:(I)I:gLoop:vLoop"
)

for major in 52 51 50; do
    mkdir -p "$WORK/orig$major/fx" "$WORK/hooked$major/fx"
    cp "$WORK/build/fx/Fx.class" "$WORK/orig$major/fx/Fx.class"
    if [ "$major" != "52" ]; then
        # The fixtures use nothing newer than Java 6, so the version can be lowered. Major 51 is the
        # strict one: StackMapTable is mandatory there, while major 50 falls back to the old verifier
        # and cannot catch a wrong frame.
        printf "$(printf '\\x00\\x%02x' "$major")" | dd of="$WORK/orig$major/fx/Fx.class" bs=1 seek=6 conv=notrunc 2>/dev/null
    fi
    "$WORK/mhook_cli" "$WORK/orig$major/fx/Fx.class" "$WORK/hooked$major/fx/Fx.class" fx/Hooks "${SPECS[@]}"
    # A second pass over rewritten bytes must be a no-op.
    "$WORK/mhook_cli" "$WORK/hooked$major/fx/Fx.class" "$WORK/hooked$major/again.class" fx/Hooks "${SPECS[@]}" | grep -q "hooked=0 already=10"
    cmp "$WORK/hooked$major/fx/Fx.class" "$WORK/hooked$major/again.class"
    echo "-- class file major $major"
    # Classes defined by a non-boot loader are verified by default; -Xverify:all makes it explicit.
    "$JAVA" -Xverify:all -cp "$WORK/build:$WORK/test" HookFixtureTest "$WORK/orig$major" "$WORK/hooked$major"
done
echo "method hook rewriter: all fixture checks passed"
