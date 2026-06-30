package com.dolphinclient.module.impl;

import com.dolphinclient.hud.HudContext;
import com.dolphinclient.module.Module;
import net.minecraft.client.Minecraft;

/** Zeigt die Spielerkoordinaten samt Blickrichtung an. */
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
        ctx.line(String.format("XYZ: %.1f / %.1f / %.1f  (%s)",
                mc.player.getX(), mc.player.getY(), mc.player.getZ(),
                facing(mc.player.getYRot())));
    }

    /** Wandelt den Gier-Winkel (yaw) in eine Himmelsrichtung. */
    private static String facing(float yaw) {
        int i = Math.floorMod((int) Math.floor((yaw + 45) / 90), 4);
        return switch (i) {
            case 0 -> "Süd";
            case 1 -> "West";
            case 2 -> "Nord";
            default -> "Ost";
        };
    }
}
