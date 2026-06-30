package com.dolphinclient.hud;

import net.minecraft.client.gui.Font;
import net.minecraft.client.gui.GuiGraphics;

/**
 * Wird pro Frame an alle aktiven HUD-Module gereicht. Stapelt Textzeilen
 * automatisch oben links (kein hartes y-Offset pro Modul mehr); für
 * grafische Module (z. B. Keystrokes) ist {@link #graphics()} direkt nutzbar.
 */
public class HudContext {
    private static final int MARGIN = 4;

    private final GuiGraphics graphics;
    private final Font font;
    private int y = MARGIN;

    public HudContext(GuiGraphics graphics, Font font) {
        this.graphics = graphics;
        this.font = font;
    }

    public GuiGraphics graphics() {
        return graphics;
    }

    public Font font() {
        return font;
    }

    /** Zeichnet eine Textzeile und rückt den Cursor nach unten. */
    public void line(String text, int color) {
        graphics.drawString(font, text, MARGIN, y, color);
        y += font.lineHeight + 2;
    }

    public void line(String text) {
        line(text, 0xFFFFFF);
    }
}
