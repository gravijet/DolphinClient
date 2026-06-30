package com.dolphinclient.module.impl;

import com.dolphinclient.hud.HudContext;
import com.dolphinclient.module.Module;
import net.minecraft.client.Minecraft;
import net.minecraft.client.Options;
import net.minecraft.client.gui.Font;
import net.minecraft.client.gui.GuiGraphicsExtractor;

/**
 * Keystrokes-Overlay (W/A/S/D + LMB/RMB) — hebt gedrückte Tasten hervor.
 * Liest die Vanilla-Tastenbelegungen aus {@link Options} (keine eigene
 * Keybind-Registrierung nötig).
 */
public class KeystrokesModule extends Module {
    private static final int SIZE = 18;
    private static final int GAP = 2;
    private static final int ORIGIN_X = 4;
    private static final int ORIGIN_Y = 120;
    private static final int DOWN = 0xAA2563EB;
    private static final int UP = 0x88000000;

    public KeystrokesModule() {
        super("keystrokes", "Keystrokes", false);
    }

    @Override
    public void onRenderHud(HudContext ctx) {
        Minecraft mc = Minecraft.getInstance();
        Options o = mc.options;
        GuiGraphicsExtractor g = ctx.graphics();
        Font font = ctx.font();

        int row2 = ORIGIN_Y + SIZE + GAP;
        int row3 = ORIGIN_Y + 2 * (SIZE + GAP);

        drawKey(g, font, ORIGIN_X + SIZE + GAP, ORIGIN_Y, SIZE, SIZE, "W", o.keyUp.isDown());
        drawKey(g, font, ORIGIN_X, row2, SIZE, SIZE, "A", o.keyLeft.isDown());
        drawKey(g, font, ORIGIN_X + SIZE + GAP, row2, SIZE, SIZE, "S", o.keyDown.isDown());
        drawKey(g, font, ORIGIN_X + 2 * (SIZE + GAP), row2, SIZE, SIZE, "D", o.keyRight.isDown());

        int wide = SIZE + (SIZE + GAP) / 2 - GAP;
        drawKey(g, font, ORIGIN_X, row3, wide, SIZE, "LMB", o.keyAttack.isDown());
        drawKey(g, font, ORIGIN_X + wide + GAP, row3, wide, SIZE, "RMB", o.keyUse.isDown());
    }

    private void drawKey(GuiGraphicsExtractor g, Font font, int x, int y, int w, int h, String label, boolean down) {
        g.fill(x, y, x + w, y + h, down ? DOWN : UP);
        int tw = font.width(label);
        g.text(font, label, x + (w - tw) / 2, y + (h - font.lineHeight) / 2 + 1, 0xFFFFFF);
    }
}
