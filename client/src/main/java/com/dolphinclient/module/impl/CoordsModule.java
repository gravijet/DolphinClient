package com.dolphinclient.module.impl;

import com.dolphinclient.hud.HudContext;
import com.dolphinclient.module.Module;
import net.minecraft.client.Minecraft;

/** Zeigt die Spielerkoordinaten an. */
public class CoordsModule extends Module {
    public CoordsModule() {
        super("coords", "Koordinaten", false);
    }

    @Override
    public void onRenderHud(HudContext ctx) {
        Minecraft mc = Minecraft.getInstance();
        if (mc.player == null) {
            return;
        }
        ctx.line(String.format("XYZ: %.1f / %.1f / %.1f",
                mc.player.getX(), mc.player.getY(), mc.player.getZ()));
    }
}
