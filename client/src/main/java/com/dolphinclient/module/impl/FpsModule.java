package com.dolphinclient.module.impl;

import com.dolphinclient.module.Module;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphics;

/** Zeigt die aktuelle Bildrate oben links an. */
public class FpsModule extends Module {
    public FpsModule() {
        super("fps", "FPS-Anzeige", true);
    }

    @Override
    public void onRenderHud(GuiGraphics graphics) {
        Minecraft mc = Minecraft.getInstance();
        String text = mc.getFps() + " FPS";
        graphics.drawString(mc.font, text, 4, 4, 0xFFFFFF);
    }
}
