package com.dolphinclient.module.impl;

import com.dolphinclient.input.DolphinKeybindings;
import com.dolphinclient.module.Module;

/**
 * Zoom: Während das Modul aktiv ist und die Zoom-Taste (Standard: C) gehalten
 * wird, verengt sich das Sichtfeld. Der eigentliche FOV-Eingriff sitzt im
 * {@code GameRendererMixin}, der {@link #isActive()} / {@link #factor()} abfragt.
 */
public class ZoomModule extends Module {
    private static final double FACTOR = 0.3;
    private static boolean enabledFlag = false;

    public ZoomModule() {
        super("zoom", "Zoom", false);
        enabledFlag = isEnabled();
    }

    @Override
    public void setEnabled(boolean enabled) {
        super.setEnabled(enabled);
        enabledFlag = enabled;
    }

    /** Aktiv, wenn das Modul an ist UND die Zoom-Taste gedrückt wird. */
    public static boolean isActive() {
        return enabledFlag
                && DolphinKeybindings.zoom != null
                && DolphinKeybindings.zoom.isDown();
    }

    /** FOV-Multiplikator (< 1 = hineinzoomen). */
    public static double factor() {
        return FACTOR;
    }
}
