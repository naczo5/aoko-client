#!/usr/bin/env python3
"""Regenerate src/main/cpp/mc_overlay_font.h from the overlay face.

The packaged release ships only aoko.exe + bridge.dll + bridge_261.dll, so the
TTF candidate list in ConfigureImGuiFontAndStyle() has nothing to load and the
HUD fell back to ImGui's ProggyClean. Embedding the face is the guaranteed
fallback; the file lookup stays first so an install can still override it.

Usage (from the repository root, on any branch that has the font):

    python3 McInjector/gen_overlay_font.py McInjector/assets/Manrope-Medium.ttf

The expected sha1 is Aoko/Data/Manrope-Medium.ttf on branch dev (the loader's copy);
McInjector/assets/Manrope-Medium.ttf is the same file. Pass --allow-unknown-hash to
embed a different file on purpose.
"""

import hashlib
import os
import sys

MANROPE_SHA1 = "20f3f6a981a8aec6cb8b30b98d0e76af21ac9f10"
# Anchored to this script so it does not matter whether it is run from the repo root
# or from McInjector/.
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "src", "main", "cpp", "mc_overlay_font.h")
BYTES_PER_LINE = 16

HEADER = """#pragma once

// Embedded overlay face: Manrope Medium, the same face the loader GUI uses.
//
// Why this exists: GetBridgeDir() is the bridge DLL's own directory, and the
// packaged zip carries only aoko.exe, bridge.dll and bridge_261.dll - no TTF and
// no Data\\ folder. Every candidate in ConfigureImGuiFontAndStyle() therefore
// missed and the in-game HUD rendered with ImGui's ProggyClean default, which is
// what "the overlay font is low quality" looked like. The lookup above still runs
// first, so an install that ships Data\\Manrope-Medium.ttf or its own face wins;
// this array is only the fallback.
//
// ImGui 1.92 keeps font sources alive for the whole lifetime of the atlas, so the
// call sites set ImFontConfig::FontDataOwnedByAtlas = false and point at this
// static storage. Never hand ImGui an owned pointer to it.
//
// Provenance: Aoko/Data/Manrope-Medium.ttf on branch dev (McInjector/assets/ mirrors it)
//   sha1 %(sha1)s, %(size)d bytes
//   licence: McInjector/assets/Manrope-OFL.md (SIL Open Font License 1.1)
// Regenerate with:
//   python3 McInjector/gen_overlay_font.py McInjector/assets/Manrope-Medium.ttf
//
// Generated file - do not hand-edit; rerun the script instead.

namespace lc {

static const unsigned char kOverlayFontTtf[] = {
"""

FOOTER = """};

// sizeof, not a literal, so the count cannot drift from the data above.
static const int kOverlayFontTtfSize = static_cast<int>(sizeof(kOverlayFontTtf));

} // namespace lc
"""


def main(argv):
    allow_unknown = "--allow-unknown-hash" in argv
    paths = [arg for arg in argv if not arg.startswith("--")]
    if len(paths) != 1:
        sys.stderr.write(__doc__)
        return 2

    with open(paths[0], "rb") as handle:
        data = handle.read()

    digest = hashlib.sha1(data).hexdigest()
    if digest != MANROPE_SHA1 and not allow_unknown:
        sys.stderr.write(
            "refusing to embed %s: sha1 %s is not the Manrope Medium the loader ships\n"
            % (paths[0], digest)
        )
        return 1

    lines = []
    for start in range(0, len(data), BYTES_PER_LINE):
        chunk = data[start:start + BYTES_PER_LINE]
        lines.append("    " + ", ".join("0x%02x" % byte for byte in chunk) + ",")

    with open(OUT, "wb") as handle:
        body = HEADER % {"sha1": digest, "size": len(data)}
        # The repository checks C++ sources out as CRLF (.gitattributes), and the
        # bridge sources already are; keep the generated file consistent.
        text = body + "\n".join(lines) + "\n" + FOOTER
        handle.write(text.replace("\n", "\r\n").encode("ascii"))

    sys.stdout.write("wrote %s (%d font bytes, %d lines)\n" % (OUT, len(data), len(lines)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
