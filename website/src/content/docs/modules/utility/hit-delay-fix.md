---
title: Hit Delay Fix
description: Removes attack delay cooldowns on Minecraft 1.8.9.
---

Hit Delay Fix removes the built-in 1.8.9 attack cooldown (`leftClickCounter`) when missing or hitting blocks, allowing attacks to register immediately on consecutive clicks.

## Overview

In Minecraft 1.8.9, missing a hit or clicking a block sets an internal click counter delay of 10 ticks (0.5 seconds), preventing subsequent hits during that interval. Hit Delay Fix resets this counter, ensuring hits are never delayed.

## Configuration Options

| Setting | Type | Description |
| :--- | :--- | :--- |
| `Hit Delay Fix` | Toggle | Enables or disables 1.8.9 hit delay removal. |
