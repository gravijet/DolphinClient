package com.dolphinclient.hud;

import com.dolphinclient.module.ModuleManager;
import net.fabricmc.fabric.api.client.rendering.v1.HudRenderCallback;

/**
 * Verbindet das HUD-Rendering von Fabric mit den aktiven Modulen.
 */
public final class HudManager {
    private HudManager() {
    }

    public static void init(ModuleManager moduleManager) {
        // Untypisierte Lambda-Parameter passen sich an die 26.1-Signatur von
        // HudRenderCallback an (erster Parameter = GuiGraphics-Render-Kontext).
        HudRenderCallback.EVENT.register((graphics, tickDelta) ->
                moduleManager.onRenderHud(graphics));
    }
}
