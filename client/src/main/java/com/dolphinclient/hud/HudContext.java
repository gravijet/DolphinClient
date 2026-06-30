package com.dolphinclient.hud;

import net.minecraft.client.gui.Font;
import net.minecraft.client.gui.GuiGraphicsExtractor;

/**
 * Wird pro Frame an alle aktiven HUD-Module gereicht. Stapelt Textzeilen
 * automatisch oben links; für grafische Module (z. B. Keystrokes) ist
 * {@link #graphics()} direkt nutzbar.
 *
 * 26.1: Der Draw-Kontext ist {@link GuiGraphicsExtractor} (ersetzt das frühere
 * GuiGraphics); Text wird über {@code text(...)} gezeichnet.
 */
public class HudContext {
    private static final int MARGIN = 4;

    private final GuiGraphicsExtractor graphics;
    private final Font font;
    private int y = MARGIN;

    public HudContext(GuiGraphicsExtractor graphics, Font font) {
        this.graphics = graphics;
        this.font = font;
    }

    public GuiGraphicsExtractor graphics() {
        return graphics;
    }

    public Font font() {
        return font;
    }

    /** Zeichnet eine Textzeile und rückt den Cursor nach unten. */
    public void line(String text, int color) {
        graphics.text(font, text, MARGIN, y, color);
        y += font.lineHeight + 2;
    }

    public void line(String text) {
        line(text, 0xFFFFFF);
    }
}
