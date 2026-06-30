package com.dolphinclient.module;

import net.minecraft.client.gui.GuiGraphics;

/**
 * Basisklasse aller DolphinClient-Module.
 *
 * Wichtig für die Performance-Garantie: Ist ein Modul deaktiviert, ruft der
 * {@link ModuleManager} weder {@link #onTick()} noch {@link #onRenderHud}
 * auf — ein ausgeschaltetes Modul kostet damit keine Laufzeit.
 */
public abstract class Module {
    private final String id;
    private final String displayName;
    private boolean enabled;

    protected Module(String id, String displayName, boolean defaultEnabled) {
        this.id = id;
        this.displayName = displayName;
        this.enabled = defaultEnabled;
    }

    public String getId() {
        return id;
    }

    public String getDisplayName() {
        return displayName;
    }

    public boolean isEnabled() {
        return enabled;
    }

    public void setEnabled(boolean enabled) {
        this.enabled = enabled;
    }

    public void toggle() {
        this.enabled = !this.enabled;
    }

    /** Pro Client-Tick — nur wenn aktiviert (ModuleManager prüft das). */
    public void onTick() {
    }

    /** Pro HUD-Frame — nur wenn aktiviert (ModuleManager prüft das). */
    public void onRenderHud(GuiGraphics graphics) {
    }
}
