package com.dolphinclient.gui;

import com.dolphinclient.module.Module;
import com.dolphinclient.module.ModuleManager;
import net.minecraft.client.gui.components.Button;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.network.chat.Component;

/**
 * In-Game-Menü zum An-/Ausschalten der Module (26.1: Screen + Button).
 */
public class DolphinMenuScreen extends Screen {
    private final ModuleManager manager;

    public DolphinMenuScreen(ModuleManager manager) {
        super(Component.literal("DolphinClient"));
        this.manager = manager;
    }

    @Override
    protected void init() {
        int y = 40;
        for (Module module : manager.getModules()) {
            Button button = Button.builder(label(module), b -> {
                manager.setEnabled(module, !module.isEnabled());
                b.setMessage(label(module));
            }).bounds(this.width / 2 - 100, y, 200, 20).build();
            this.addRenderableWidget(button);
            y += 24;
        }

        this.addRenderableWidget(Button.builder(Component.literal("Schließen"), b -> this.onClose())
                .bounds(this.width / 2 - 100, y + 8, 200, 20).build());
    }

    private Component label(Module module) {
        return Component.literal(module.getDisplayName() + ": " + (module.isEnabled() ? "AN" : "AUS"));
    }

    @Override
    public boolean isPauseScreen() {
        return false;
    }
}
