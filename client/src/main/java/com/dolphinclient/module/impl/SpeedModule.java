package com.dolphinclient.module.impl;

import com.dolphinclient.hud.HudContext;
import com.dolphinclient.module.Module;
import net.minecraft.client.Minecraft;

/**
 * Horizontale Geschwindigkeit in Blöcken/Sekunde.
 *
 * API-Berührungspunkte (Mojang-Namen, gegen 26.1-Quelle verifizieren):
 * {@code LocalPlayer.getDeltaMovement()} -> {@code Vec3.horizontalDistance()};
 * 20 Ticks pro Sekunde.
 */
public class SpeedModule extends Module {
    public SpeedModule() {
        super("speed", "Geschwindigkeit", false);
    }

    @Override
    public void onRenderHud(HudContext ctx) {
        Minecraft mc = Minecraft.getInstance();
        if (mc.player == null) {
            return;
        }
        double blocksPerSecond = mc.player.getDeltaMovement().horizontalDistance() * 20.0;
        ctx.line(String.format("Speed: %.2f b/s", blocksPerSecond));
    }
}
