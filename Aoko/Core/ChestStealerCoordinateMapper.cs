using System;

namespace Aoko.Core;

public static class ChestStealerCoordinateMapper
{
    public static bool TryMapScaledPoint(
        ChestStealerState state,
        ChestStealerSlot slot,
        WindowDetection.RECT clientRect,
        out int screenX,
        out int screenY)
    {
        screenX = 0;
        screenY = 0;

        int clientWidth = clientRect.Right - clientRect.Left;
        int clientHeight = clientRect.Bottom - clientRect.Top;
        if (state.ScreenWidth <= 0 || state.ScreenHeight <= 0 || clientWidth <= 0 || clientHeight <= 0)
            return false;

        double scaleX = clientWidth / (double)state.ScreenWidth;
        double scaleY = clientHeight / (double)state.ScreenHeight;
        screenX = clientRect.Left + (int)Math.Round(slot.X * scaleX);
        screenY = clientRect.Top + (int)Math.Round(slot.Y * scaleY);
        return true;
    }
}
