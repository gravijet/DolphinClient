package com.dolphinclient.module.impl;

import com.dolphinclient.input.DolphinKeybindings;
import com.dolphinclient.module.Module;
import net.minecraft.client.Minecraft;
import net.minecraft.client.Options;

/**
 * Zoom: Während das Modul aktiv ist und die Zoom-Taste (C) gehalten wird,
 * wird das Sichtfeld temporär verkleinert.
 *
 * 26.1: kein FOV-Render-Hook mehr — stattdessen wird die FOV-Option
 * ({@link Options#fov()}) beim Halten gesetzt und beim Loslassen (oder beim
 * Deaktivieren) wiederhergestellt.
 */
public class ZoomModule extends Module {
    private static final double FACTOR = 0.3;
    private Integer originalFov = null;

    public ZoomModule() {
        super("zoom", "Zoom", false);
    }

    @Override
    public void onTick() {
        Options options = Minecraft.getInstance().options;
        boolean keyDown = DolphinKeybindings.zoom != null && DolphinKeybindings.zoom.isDown();

        if (keyDown && originalFov == null) {
            originalFov = options.fov().get();
            int reduced = Math.max(1, (int) Math.round(originalFov * FACTOR));
            options.fov().set(reduced);
        } else if (!keyDown && originalFov != null) {
            options.fov().set(originalFov);
            originalFov = null;
        }
    }

    @Override
    public void setEnabled(boolean enabled) {
        super.setEnabled(enabled);
        // Wird das Modul während des Zoomens ausgeschaltet: FOV wiederherstellen.
        if (!enabled && originalFov != null) {
            Minecraft.getInstance().options.fov().set(originalFov);
            originalFov = null;
        }
    }
}
