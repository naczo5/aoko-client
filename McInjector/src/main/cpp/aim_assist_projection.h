#pragma once

#include <cmath>

namespace lc {
namespace aimassist {

/// Selects the projected player-body sample nearest the viewport crosshair.
/// The projector must have the signature:
/// bool(double worldX, double worldY, double worldZ, float* screenX, float* screenY).
template <typename Projector>
inline bool SelectClosestBodyPoint(
    double baseX,
    double baseY,
    double baseZ,
    int viewportWidth,
    int viewportHeight,
    Projector projector,
    float* outScreenX,
    float* outScreenY)
{
    if (viewportWidth <= 0 || viewportHeight <= 0 || !outScreenX || !outScreenY)
        return false;

    static const double xOffsets[] = { -0.30, 0.0, 0.30 };
    static const double zOffsets[] = { -0.30, 0.0, 0.30 };
    static const double yOffsets[] = { 0.15, 0.55, 0.95, 1.35, 1.75 };

    const double centerX = viewportWidth * 0.5;
    const double centerY = viewportHeight * 0.5;
    double bestScore = 1e30;
    float bestX = 0.0f;
    float bestY = 0.0f;
    bool found = false;

    for (double yOffset : yOffsets) {
        for (double xOffset : xOffsets) {
            for (double zOffset : zOffsets) {
                float screenX = 0.0f;
                float screenY = 0.0f;
                if (!projector(
                        baseX + xOffset,
                        baseY + yOffset,
                        baseZ + zOffset,
                        &screenX,
                        &screenY)) {
                    continue;
                }
                if (!std::isfinite(screenX) || !std::isfinite(screenY))
                    continue;

                const double dx = screenX - centerX;
                const double dy = screenY - centerY;
                const double score = dx * dx + dy * dy;
                // Strict comparison intentionally preserves the first sample on ties.
                if (score < bestScore) {
                    bestScore = score;
                    bestX = screenX;
                    bestY = screenY;
                    found = true;
                }
            }
        }
    }

    if (!found)
        return false;

    *outScreenX = bestX;
    *outScreenY = bestY;
    return true;
}

} // namespace aimassist
} // namespace lc
