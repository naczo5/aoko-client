#pragma once

#include <cmath>

namespace lc {

// GameRenderer.projectPointToScreen returns normalized device coordinates, not
// framebuffer pixels. Keep the conversion here so every JNI snapshot consumer
// shares the same coordinate contract.
struct ScreenProjection {
    float x = 0.0f;
    float y = 0.0f;
};

inline bool ConvertNdcProjectionToViewport(double ndcX, double ndcY, double ndcZ,
                                           int viewportWidth, int viewportHeight,
                                           ScreenProjection* out)
{
    if (!out || viewportWidth <= 1 || viewportHeight <= 1) return false;
    if (!std::isfinite(ndcX) || !std::isfinite(ndcY) || !std::isfinite(ndcZ)) return false;

    // OpenGL NDC: x/y outside this range cannot appear in the viewport and a
    // z value outside the clip volume is either behind the camera or clipped.
    if (ndcX < -1.0 || ndcX > 1.0 || ndcY < -1.0 || ndcY > 1.0 ||
        ndcZ < -1.0 || ndcZ > 1.0) return false;

    const double pixelX = (ndcX + 1.0) * 0.5 * static_cast<double>(viewportWidth);
    const double pixelY = (1.0 - ndcY) * 0.5 * static_cast<double>(viewportHeight);
    if (!std::isfinite(pixelX) || !std::isfinite(pixelY)) return false;

    out->x = static_cast<float>(pixelX);
    out->y = static_cast<float>(pixelY);
    return std::isfinite(out->x) && std::isfinite(out->y);
}

} // namespace lc
