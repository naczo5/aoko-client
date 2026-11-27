#!/usr/bin/env bash
# McInjector/gen_visuals_helper.sh
#
# Regenerates src/main/cpp/jni_core/visuals_helper.inc from the sources under
# src/main/java/com/aoko/visuals.
#
# The bridges embed every class of com.aoko.visuals as a byte array and define them in the
# game's ClassLoader (see visuals_bridge.cpp), so the output must be Java 8 bytecode
# (class file major version 52). LWJGL 2 is only needed to compile LegacyGl, so it is
# compiled against the stubs in tools/lwjgl2-stubs and the stubs are never embedded.
#
# Output per class, in definition order (interfaces first so implementers can follow):
#   static const unsigned char kVisualsClassN[] = { ... };
# and a table:
#   static const VisualsClassEntry kVisualsClasses[] = { { "com/aoko/visuals/X", kVisualsClassN, len }, ... };
#
# Usage:
#   ./gen_visuals_helper.sh            # rewrite the .inc in place
#   ./gen_visuals_helper.sh --check    # exit 1 when the committed .inc is stale
#
# Requires bash, javac (JDK 9+ for --release 8), find, sort, od and awk.
set -euo pipefail

cd "$(dirname "$0")"

JAVAC="${JAVAC:-javac}"
SRC_DIR="src/main/java/com/aoko/visuals"
STUB_DIR="tools/lwjgl2-stubs"
TARGET="src/main/cpp/jni_core/visuals_helper.inc"
CHECK_ONLY=0

if [ "${1:-}" = "--check" ]; then
    CHECK_ONLY=1
elif [ "$#" -ne 0 ]; then
    echo "usage: $0 [--check]" >&2
    exit 2
fi

OUT_DIR="$(mktemp -d)"
STAGE_FILE="$OUT_DIR/visuals_helper.inc"
cleanup() { rm -rf "$OUT_DIR"; }
trap cleanup EXIT

SOURCES=()
while IFS= read -r f; do SOURCES+=("$f"); done < <(find "$SRC_DIR" -name '*.java' | LC_ALL=C sort)
STUBS=()
while IFS= read -r f; do STUBS+=("$f"); done < <(find "$STUB_DIR" -name '*.java' | LC_ALL=C sort)

# -g:none keeps the payload small and the output stable; --release 8 pins the bytecode level.
"$JAVAC" --release 8 -g:none -Xlint:all,-options -d "$OUT_DIR/classes" "${SOURCES[@]}" "${STUBS[@]}"

# Definition order: the two interfaces first, then everything else sorted by name.
FIRST=("com/aoko/visuals/Platform" "com/aoko/visuals/QuadSink")
ORDERED=()
for n in "${FIRST[@]}"; do ORDERED+=("$n"); done
while IFS= read -r f; do
    rel="${f#"$OUT_DIR/classes/"}"
    rel="${rel%.class}"
    skip=0
    for n in "${FIRST[@]}"; do [ "$rel" = "$n" ] && skip=1; done
    [ "$skip" -eq 0 ] && ORDERED+=("$rel")
done < <(find "$OUT_DIR/classes/com/aoko/visuals" -name '*.class' | LC_ALL=C sort)

{
    echo "// Auto-generated from com.aoko.visuals - do not edit manually."
    echo "// Regenerate: McInjector/gen_visuals_helper.sh (javac --release 8 -g:none)."
    echo "// Source: McInjector/$SRC_DIR"
    echo "struct VisualsClassEntry {"
    echo "    const char* name;"
    echo "    const unsigned char* bytes;"
    echo "    int length;"
    echo "};"
    index=0
    for rel in "${ORDERED[@]}"; do
        class_file="$OUT_DIR/classes/$rel.class"
        if [ ! -f "$class_file" ]; then
            echo "error: javac did not produce $class_file" >&2
            exit 1
        fi
        header="$(od -An -v -tx1 -N8 "$class_file" | tr -s ' ' | sed 's/^ //; s/ $//')"
        if [ "$header" != "ca fe ba be 00 00 00 34" ]; then
            echo "error: $rel has class header ($header); expected CAFEBABE major 52 (Java 8)" >&2
            exit 1
        fi
        echo "static const unsigned char kVisualsClass$index[] = {"
        od -An -v -tu1 "$class_file" | awk '
            { for (i = 1; i <= NF; i++) bytes[++count] = $i }
            END {
                for (start = 1; start <= count; start += 12) {
                    last = (start + 11 <= count) ? start + 11 : count
                    line = "   "
                    for (j = start; j <= last; j++) {
                        line = line sprintf(" 0x%02X", bytes[j])
                        if (j < count) line = line ","
                    }
                    print line
                }
            }
        '
        echo "};"
        index=$((index + 1))
    done
    echo "static const VisualsClassEntry kVisualsClasses[] = {"
    index=0
    for rel in "${ORDERED[@]}"; do
        len="$(wc -c < "$OUT_DIR/classes/$rel.class" | tr -d ' ')"
        echo "    { \"$rel\", kVisualsClass$index, $len },"
        index=$((index + 1))
    done
    echo "};"
    echo "static const int kVisualsClassCount = $index;"
} > "$STAGE_FILE"

if [ "$CHECK_ONLY" -eq 1 ]; then
    # The repository checks text files out with CRLF, so compare ignoring CR.
    if diff -q <(tr -d '\r' < "$TARGET" 2>/dev/null) <(tr -d '\r' < "$STAGE_FILE") > /dev/null 2>&1; then
        echo "$TARGET is up to date (${#ORDERED[@]} classes)."
        exit 0
    fi
    echo "error: $TARGET is stale; rerun ./gen_visuals_helper.sh" >&2
    exit 1
fi

cp "$STAGE_FILE" "$TARGET"
echo "Wrote $TARGET (${#ORDERED[@]} classes, $(wc -c < "$TARGET" | tr -d ' ') bytes of source)."
