package com.dolphinclient.module.impl;

import com.dolphinclient.module.Module;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphics;

/**
 * Klicks pro Sekunde (CPS).
 *
 * Skelett: Das echte Zählen der Mausklicks erfordert einen Mixin in den
 * Maus-Handler von 26.1 (M1). Vorerst Platzhalterwert.
 */
public class CpsModule extends Module {
    public CpsModule() {
        super("cps", "CPS-Anzeige", false);
    }

    @Override
    public void onRenderHud(GuiGraphics graphics) {
        Minecraft mc = Minecraft.getInstance();
        // TODO(M1): tatsächliche Klickrate aus einem Maus-Mixin beziehen.
        graphics.drawString(mc.font, "CPS: 0", 4, 24, 0xFFFFFF);
    }
}
