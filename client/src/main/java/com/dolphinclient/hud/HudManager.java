package com.dolphinclient.hud;

import com.dolphinclient.module.ModuleManager;
import net.fabricmc.fabric.api.client.rendering.v1.hud.HudElementRegistry;
import net.minecraft.resources.Identifier;

/**
 * Registriert ein HUD-Element (26.1: {@link HudElementRegistry}) und leitet das
 * Rendern an die aktiven Module weiter.
 */
public final class HudManager {
    private HudManager() {
    }

    public static void init(ModuleManager moduleManager) {
        // HudElement ist ein funktionales Interface:
        // extractRenderState(GuiGraphicsExtractor, DeltaTracker).
        HudElementRegistry.addLast(
                Identifier.fromNamespaceAndPath("dolphinclient", "hud"),
                (graphics, deltaTracker) -> moduleManager.onRenderHud(graphics));
    }
}
