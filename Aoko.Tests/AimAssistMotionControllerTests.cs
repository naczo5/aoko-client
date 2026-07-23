using Aoko.Core;

namespace Aoko.Tests;

public class AimAssistMotionControllerTests
{
    [Fact]
    public void EquivalentTelemetry_ProducesEquivalentMovementWithoutVersionInput()
    {
        var legacy = new AimAssistMotionController();
        var modern = new AimAssistMotionController();
        List<EntityInfo> entities = [Entity("Player", 1030, 520)];

        AimAssistMotionResult legacyMove = Step(legacy, entities);
        AimAssistMotionResult modernMove = Step(modern, entities);

        Assert.Equal(legacyMove, modernMove);
        Assert.True(legacyMove.HasTarget);
    }

    [Theory]
    [InlineData(10)]
    [InlineData(50)]
    [InlineData(100)]
    public void Strength_AlwaysProducesBoundedModernBaselineMovement(int strength)
    {
        var controller = new AimAssistMotionController();

        AimAssistMotionResult move = Step(
            controller,
            [Entity("Player", 1400, 800)],
            strength: strength,
            aimFov: 120);

        int expectedMax = (int)Math.Round(4.0 + 16.0 * strength / 100.0);
        Assert.True(move.HasTarget);
        Assert.InRange(Math.Abs(move.MoveX), 0, expectedMax);
        Assert.InRange(Math.Abs(move.MoveY), 0, expectedMax);
    }

    [Theory]
    [InlineData(30, true)]
    [InlineData(70, false)]
    [InlineData(110, false)]
    public void ReportedGameFov_ControlsAngularEligibility(float gameFov, bool expectedTarget)
    {
        var controller = new AimAssistMotionController();

        AimAssistMotionResult move = Step(
            controller,
            [Entity("Player", 1180, 540)],
            gameFov: gameFov,
            aimFov: 30);

        Assert.Equal(expectedTarget, move.HasTarget);
    }

    [Fact]
    public void InvalidGameFov_UsesSafeDefault()
    {
        List<EntityInfo> entities = [Entity("Player", 1040, 540)];

        AimAssistMotionResult invalid = Step(new AimAssistMotionController(), entities, gameFov: 0);
        AimAssistMotionResult fallback = Step(new AimAssistMotionController(), entities, gameFov: 70);

        Assert.Equal(fallback, invalid);
    }

    [Fact]
    public void ResolutionScaling_PreservesEquivalentAngularTargeting()
    {
        AimAssistMotionResult fullHd = Step(
            new AimAssistMotionController(),
            [Entity("Player", 1060, 540)],
            width: 1920,
            height: 1080,
            aimFov: 20);
        AimAssistMotionResult qhd = Step(
            new AimAssistMotionController(),
            [Entity("Player", 1413.333f, 720)],
            width: 2560,
            height: 1440,
            aimFov: 20);

        Assert.True(fullHd.HasTarget);
        Assert.True(qhd.HasTarget);
        Assert.Equal(Math.Sign(fullHd.MoveX), Math.Sign(qhd.MoveX));
    }

    [Fact]
    public void RangeAndScreenBounds_ExcludeInvalidTargets()
    {
        var controller = new AimAssistMotionController();
        List<EntityInfo> entities =
        [
            Entity("TooFar", 970, 540, 8),
            Entity("Offscreen", -1, 540),
        ];

        AimAssistMotionResult move = Step(controller, entities, range: 4.5f);

        Assert.False(move.HasTarget);
        Assert.Equal(0, move.MoveX);
        Assert.Equal(0, move.MoveY);
    }

    [Fact]
    public void LookingAtEntity_SuppressesAndClearsResidualMovement()
    {
        List<EntityInfo> entities = [Entity("Player", 1080, 540)];
        var controller = new AimAssistMotionController();
        _ = Step(controller, entities);
        _ = Step(controller, entities);

        AimAssistMotionResult suppressed = Step(controller, entities, lookingAtEntity: true);
        AimAssistMotionResult reacquired = Step(controller, entities);
        AimAssistMotionResult fresh = Step(new AimAssistMotionController(), entities);

        Assert.True(suppressed.Suppressed);
        Assert.False(suppressed.HasTarget);
        Assert.Equal(0, suppressed.MoveX);
        Assert.Equal(0, suppressed.MoveY);
        Assert.Equal(fresh, reacquired);
    }

    [Fact]
    public void LatchedStateAlone_DoesNotSuppressImmediateReacquisition()
    {
        var state = new GameState
        {
            LookingAtEntity = false,
            LookingAtEntityLatched = true,
            Entities = [Entity("Player", 1080, 540)],
        };

        AimAssistMotionResult move = Step(
            new AimAssistMotionController(),
            state.Entities,
            lookingAtEntity: state.LookingAtEntity);

        Assert.False(move.Suppressed);
        Assert.True(move.HasTarget);
        Assert.NotEqual(0, move.MoveX);
    }

    [Fact]
    public void TargetHandoff_DoesNotCarryPreviousDirection()
    {
        var controller = new AimAssistMotionController();
        List<EntityInfo> first = [Entity("Alpha", 1160, 540)];
        _ = Step(controller, first);
        _ = Step(controller, first);
        _ = Step(controller, first);

        AimAssistMotionResult switched = Step(controller, [Entity("Beta", 900, 540)]);
        AimAssistMotionResult fresh = Step(new AimAssistMotionController(), [Entity("Beta", 900, 540)]);

        Assert.Equal("Beta", switched.TargetName);
        Assert.Equal(fresh.MoveX, switched.MoveX);
        Assert.True(switched.MoveX < 0);
    }

    [Fact]
    public void SameMovingTarget_PreservesSmoothingDirection()
    {
        var controller = new AimAssistMotionController();

        AimAssistMotionResult first = Step(controller, [Entity("Player", 1080, 540)]);
        AimAssistMotionResult moved = Step(controller, [Entity("Player", 1100, 540)]);

        Assert.Equal("Player", moved.TargetName);
        Assert.True(first.MoveX > 0);
        Assert.True(moved.MoveX >= first.MoveX);
    }

    [Fact]
    public void NoEligibleTarget_ResetsTrackingState()
    {
        var controller = new AimAssistMotionController();
        List<EntityInfo> target = [Entity("Player", 1080, 540)];
        _ = Step(controller, target);
        _ = Step(controller, target);

        AimAssistMotionResult none = Step(controller, []);
        AimAssistMotionResult reacquired = Step(controller, target);
        AimAssistMotionResult fresh = Step(new AimAssistMotionController(), target);

        Assert.False(none.HasTarget);
        Assert.Equal(fresh, reacquired);
    }

    private static AimAssistMotionResult Step(
        AimAssistMotionController controller,
        IReadOnlyList<EntityInfo> entities,
        int width = 1920,
        int height = 1080,
        float gameFov = 70,
        float aimFov = 30,
        float range = 4.5f,
        int strength = 40,
        bool lookingAtEntity = false)
        => controller.Update(
            entities,
            width,
            height,
            gameFov,
            aimFov,
            range,
            strength,
            lookingAtEntity);

    private static EntityInfo Entity(string name, float sx, float sy, double dist = 3)
        => new() { Name = name, Sx = sx, Sy = sy, Dist = dist };
}
