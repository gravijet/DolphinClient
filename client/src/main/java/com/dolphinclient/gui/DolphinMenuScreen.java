package com.dolphinclient.gui;

import com.dolphinclient.module.Module;
import com.dolphinclient.module.ModuleManager;
import net.minecraft.ChatFormatting;
import net.minecraft.client.gui.GuiGraphicsExtractor;
import net.minecraft.client.gui.components.Button;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.network.chat.Component;

/**
 * In-Game-Menü zum An-/Ausschalten der Module (26.1: Screen + Button,
 * gezeichnet über {@code extractRenderState(GuiGraphicsExtractor, ...)}).
 *
 * Kopfzeile mit Titel/Untertitel + Trennlinie, „Alle an/aus"-Schnellschalter,
 * pro Modul ein Button mit farbig hervorgehobenem Zustand, Fußzeile.
 */
public class DolphinMenuScreen extends Screen {
    private static final int ACCENT = 0xFF4AA3FF;
    private static final int MUTED = 0xFFA0A0B0;
    private static final int BUTTON_WIDTH = 220;
    private static final int BUTTON_HEIGHT = 20;
    private static final int ROW = 24;
    private static final int HEADER_BOTTOM = 44;

    private final ModuleManager manager;

    public DolphinMenuScreen(ModuleManager manager) {
        super(Component.literal("DolphinClient"));
        this.manager = manager;
    }

    @Override
    protected void init() {
        final int x = this.width / 2 - BUTTON_WIDTH / 2;
        int y = HEADER_BOTTOM + 10;

        // Schnellschalter: Alle an / Alle aus.
        int half = (BUTTON_WIDTH - 4) / 2;
        this.addRenderableWidget(Button.builder(Component.literal("Alle an"), b -> setAll(true))
                .bounds(x, y, half, BUTTON_HEIGHT).build());
        this.addRenderableWidget(Button.builder(Component.literal("Alle aus"), b -> setAll(false))
                .bounds(x + half + 4, y, half, BUTTON_HEIGHT).build());
        y += ROW + 6;

        // Ein Button pro Modul.
        for (Module module : manager.getModules()) {
            Button button = Button.builder(label(module), b -> {
                manager.setEnabled(module, !module.isEnabled());
                b.setMessage(label(module));
            }).bounds(x, y, BUTTON_WIDTH, BUTTON_HEIGHT).build();
            this.addRenderableWidget(button);
            y += ROW;
        }

        y += 6;
        this.addRenderableWidget(Button.builder(Component.literal("Schließen"), b -> this.onClose())
                .bounds(x, y, BUTTON_WIDTH, BUTTON_HEIGHT).build());
    }

    @Override
    public void extractRenderState(GuiGraphicsExtractor graphics, int mouseX, int mouseY, float partialTick) {
        // Zeichnet den (abgedunkelten) Hintergrund + alle Widgets (Buttons).
        super.extractRenderState(graphics, mouseX, mouseY, partialTick);

        // Kopfzeile.
        graphics.centeredText(this.font, this.title, this.width / 2, 16, ACCENT);
        graphics.centeredText(this.font,
                Component.literal("Module — klicken zum Umschalten"),
                this.width / 2, 30, MUTED);

        // Trennlinie unter dem Kopf.
        int lineX = this.width / 2 - BUTTON_WIDTH / 2;
        graphics.fill(lineX, HEADER_BOTTOM, lineX + BUTTON_WIDTH, HEADER_BOTTOM + 1, 0x40FFFFFF);

        // Fußzeile.
        graphics.centeredText(this.font,
                Component.literal("DolphinClient v0.1.0 · Minecraft 26.1"),
                this.width / 2, this.height - 16, MUTED);
    }

    private void setAll(boolean enabled) {
        for (Module module : manager.getModules()) {
            manager.setEnabled(module, enabled);
        }
        this.rebuildWidgets();
    }

    private Component label(Module module) {
        boolean on = module.isEnabled();
        return Component.literal(module.getDisplayName() + ": ")
                .append(Component.literal(on ? "AN" : "AUS")
                        .withStyle(on ? ChatFormatting.GREEN : ChatFormatting.GRAY));
    }

    @Override
    public boolean isPauseScreen() {
        return false;
    }
}
