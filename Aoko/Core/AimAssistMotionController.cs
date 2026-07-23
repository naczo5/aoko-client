using System;
using System.Collections.Generic;

namespace Aoko.Core;

internal readonly record struct AimAssistMotionResult(
    bool HasTarget,
    bool Suppressed,
    int MoveX,
    int MoveY,
    string? TargetName);

/// <summary>
/// Selects an aim-assist target and converts its screen-space error into a
/// bounded relative mouse movement. This controller is shared by every bridge.
/// </summary>
internal sealed class AimAssistMotionController
{
    private const double FilterAlpha = 0.40;
    private readonly object _sync = new();
    private double _filteredDx;
    private double _filteredDy;
    private string? _targetName;

    public AimAssistMotionResult Update(
        IReadOnlyList<EntityInfo> entities,
        int width,
        int height,
        float gameFov,
        float aimAssistFov,
        float aimAssistRange,
        int aimAssistStrength,
        bool lookingAtEntity)
    {
        lock (_sync)
        {
            if (lookingAtEntity)
            {
                ResetCore();
                return new AimAssistMotionResult(false, true, 0, 0, null);
            }

            if (entities.Count == 0 || width <= 0 || height <= 0)
            {
                ResetCore();
                return default;
            }

            double safeGameFov = gameFov is >= 10.0f and <= 170.0f ? gameFov : 70.0;
            double focalLengthPx = height / (2.0 * Math.Tan(safeGameFov * Math.PI / 360.0));
            focalLengthPx = Math.Max(1.0, focalLengthPx);

            double centerX = width * 0.5;
            double centerY = height * 0.5;
            double maxAngle = Math.Clamp(aimAssistFov, 1.0f, 180.0f) * 0.5;
            double range = Math.Max(0.0, aimAssistRange);
            double bestScore = double.MaxValue;
            double bestDx = 0.0;
            double bestDy = 0.0;
            string? bestTargetName = null;

            for (int index = 0; index < entities.Count; index++)
            {
                EntityInfo entity = entities[index];
                if (entity.Dist <= 0.01 || entity.Dist > range)
                    continue;
                if (entity.Sx < 0 || entity.Sx > width || entity.Sy < 0 || entity.Sy > height)
                    continue;

                double dx = entity.Sx - centerX;
                double dy = entity.Sy - centerY;
                double radial = Math.Sqrt(dx * dx + dy * dy);
                double angle = Math.Atan2(radial, focalLengthPx) * 180.0 / Math.PI;
                if (angle > maxAngle)
                    continue;

                double score = dx * dx + dy * dy;
                if (score < bestScore)
                {
                    bestScore = score;
                    bestDx = dx;
                    bestDy = dy;
                    bestTargetName = string.IsNullOrEmpty(entity.Name)
                        ? $"#{index}"
                        : entity.Name;
                }
            }

            if (bestScore == double.MaxValue)
            {
                ResetCore();
                return default;
            }

            if (!string.Equals(_targetName, bestTargetName, StringComparison.Ordinal))
            {
                _filteredDx = 0.0;
                _filteredDy = 0.0;
                _targetName = bestTargetName;
            }

            _filteredDx += (bestDx - _filteredDx) * FilterAlpha;
            _filteredDy += (bestDy - _filteredDy) * FilterAlpha;

            double strengthNorm = Math.Clamp(aimAssistStrength, 0, 100) / 100.0;
            double strength = 0.08 + 0.36 * strengthNorm * strengthNorm;
            int moveX = (int)Math.Round(_filteredDx * strength);
            int moveY = (int)Math.Round(_filteredDy * strength);

            if (moveX == 0 && Math.Abs(bestDx) > 1.0)
                moveX = Math.Sign(bestDx);
            if (moveY == 0 && Math.Abs(bestDy) > 1.0)
                moveY = Math.Sign(bestDy);

            int maxStep = (int)Math.Round(4.0 + 16.0 * strengthNorm);
            moveX = Math.Clamp(moveX, -maxStep, maxStep);
            moveY = Math.Clamp(moveY, -maxStep, maxStep);

            return new AimAssistMotionResult(true, false, moveX, moveY, bestTargetName);
        }
    }

    public void Reset()
    {
        lock (_sync)
            ResetCore();
    }

    private void ResetCore()
    {
        _filteredDx = 0.0;
        _filteredDy = 0.0;
        _targetName = null;
    }
}
