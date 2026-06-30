package com.dolphinclient.module.impl;

import com.dolphinclient.module.Module;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphics;

/** Zeigt die Spielerkoordinaten an. */
public class CoordsModule extends Module {
    public CoordsModule() {
        super("coords", "Koordinaten", false);
    }

    @Override
    public void onRenderHud(GuiGraphics graphics) {
        Minecraft mc = Minecraft.getInstance();
        if (mc.player == null) {
            return;
        }
        String text = String.format("XYZ: %.1f / %.1f / %.1f",
                mc.player.getX(), mc.player.getY(), mc.player.getZ());
        graphics.drawString(mc.font, text, 4, 14, 0xFFFFFF);
    }
}
