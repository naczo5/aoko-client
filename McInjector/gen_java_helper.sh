#!/usr/bin/env bash
# McInjector/gen_java_helper.sh
#
# Regenerates src/main/cpp/jni_core/session_switcher.inc from
# src/main/java/com/aoko/helper/SessionSwitcher.java.
#
# The bridge embeds the helper as a byte array and defines it inside the game JVM
# with ClassLoader.defineClass(), so the emitted class must stay Java 8 bytecode
# (class file major version 52) and its name must match what the loader passes to
# defineClass: com.aoko.helper.SessionSwitcher (the dotted binary name; the
# slashed form is only ever used for a class-file path).
#
# Output format matches the existing jni_core/aoko_helper.inc:
#   static const unsigned char kSessionSwitcherClassBytes[] = { ... };
#   static const int kSessionSwitcherClassLen = <bytes>;
# Twelve bytes per line, hex uppercase, no trailing comma after the final byte.
#
# Usage:
#   ./gen_java_helper.sh            # rewrite the .inc in place
#   ./gen_java_helper.sh --check    # exit 1 when the committed .inc is stale
#
# Requires bash, javac (JDK 8 or newer; --release 8 needs JDK 9+) and od/awk.
set -euo pipefail

cd "$(dirname "$0")"

JAVAC="${JAVAC:-javac}"
SOURCE="src/main/java/com/aoko/helper/SessionSwitcher.java"
CLASS_NAME="com/aoko/helper/SessionSwitcher"
TARGET="src/main/cpp/jni_core/session_switcher.inc"
CHECK_ONLY=0

if [ "${1:-}" = "--check" ]; then
    CHECK_ONLY=1
elif [ "$#" -ne 0 ]; then
    echo "usage: $0 [--check]" >&2
    exit 2
fi

if [ ! -f "$SOURCE" ]; then
    echo "error: $SOURCE not found (run from McInjector/)" >&2
    exit 1
fi

OUT_DIR="$(mktemp -d)"
STAGE_FILE="$OUT_DIR/session_switcher.inc"
cleanup() { rm -rf "$OUT_DIR"; }
trap cleanup EXIT

# -g:none keeps the payload small and the output stable across builds; --release 8
# guarantees Java 8 bytecode so the class loads in every supported game JVM.
"$JAVAC" --release 8 -g:none -Xlint:all,-options -d "$OUT_DIR" "$SOURCE"

CLASS_FILE="$OUT_DIR/$CLASS_NAME.class"
if [ ! -f "$CLASS_FILE" ]; then
    echo "error: javac did not produce $CLASS_FILE" >&2
    exit 1
fi

MAGIC_AND_VERSION="$(od -An -v -tx1 -N8 "$CLASS_FILE" | tr -s ' ' | sed 's/^ //; s/ $//')"
if [ "$MAGIC_AND_VERSION" != "ca fe ba be 00 00 00 34" ]; then
    echo "error: unexpected class header ($MAGIC_AND_VERSION); expected CAFEBABE major 52 (Java 8)" >&2
    exit 1
fi

CLASS_LEN="$(wc -c < "$CLASS_FILE" | tr -d ' ')"
if [ "$CLASS_LEN" -eq 0 ]; then
    echo "error: empty class file" >&2
    exit 1
fi

{
    echo "// Auto-generated from $CLASS_NAME.class - do not edit manually."
    echo "// Regenerate: McInjector/gen_java_helper.sh (javac --release 8 -g:none)."
    echo "// Source: McInjector/$SOURCE"
    echo "static const unsigned char kSessionSwitcherClassBytes[] = {"
    od -An -v -tu1 "$CLASS_FILE" | awk '
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
    echo "static const int kSessionSwitcherClassLen = $CLASS_LEN;"
} > "$STAGE_FILE"

if [ "$CHECK_ONLY" -eq 1 ]; then
    # The repository checks text files out with CRLF, so compare ignoring CR.
    if diff -q <(tr -d '\r' < "$TARGET" 2>/dev/null) <(tr -d '\r' < "$STAGE_FILE") > /dev/null 2>&1; then
        echo "$TARGET is up to date ($CLASS_LEN bytes)."
        exit 0
    fi
    echo "error: $TARGET is stale; rerun ./gen_java_helper.sh" >&2
    diff -u <(tr -d '\r' < "$TARGET" 2>/dev/null) <(tr -d '\r' < "$STAGE_FILE") | head -40 >&2 || true
    exit 1
fi

cp "$STAGE_FILE" "$TARGET"
echo "Wrote $TARGET ($CLASS_LEN bytes)."
