package com.dolphinclient.module;

import com.dolphinclient.config.DolphinConfig;
import com.dolphinclient.hud.HudContext;
import com.dolphinclient.module.impl.ClockModule;
import com.dolphinclient.module.impl.CoordsModule;
import com.dolphinclient.module.impl.FpsModule;
import com.dolphinclient.module.impl.KeystrokesModule;
import com.dolphinclient.module.impl.SessionTimeModule;
import com.dolphinclient.module.impl.SpeedModule;
import com.dolphinclient.module.impl.ZoomModule;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiGraphicsExtractor;

import java.util.ArrayList;
import java.util.List;

/**
 * Registry und Lebenszyklus der Module. Schaltet Module an/aus, persistiert
 * den Zustand über {@link DolphinConfig} und ruft nur aktive Module auf.
 */
public class ModuleManager {
    private final List<Module> modules = new ArrayList<>();
    private final DolphinConfig config;

    public ModuleManager(DolphinConfig config) {
        this.config = config;
    }

    public void registerDefaults() {
        register(new FpsModule());
        register(new CoordsModule());
        register(new ClockModule());
        register(new SessionTimeModule());
        register(new SpeedModule());
        register(new KeystrokesModule());
        register(new ZoomModule());
        // Weitere Module hier registrieren.
    }

    public void register(Module module) {
        // Gespeicherten An/Aus-Zustand wiederherstellen, falls vorhanden.
        Boolean saved = config.modules.get(module.getId());
        if (saved != null) {
            module.setEnabled(saved);
        }
        modules.add(module);
    }

    public List<Module> getModules() {
        return modules;
    }

    public void setEnabled(Module module, boolean enabled) {
        module.setEnabled(enabled);
        config.modules.put(module.getId(), enabled);
        config.save();
    }

    /** Nur aktive Module laufen — deaktivierte kosten nichts. */
    public void onTick() {
        for (Module m : modules) {
            if (m.isEnabled()) {
                m.onTick();
            }
        }
    }

    public void onRenderHud(GuiGraphicsExtractor graphics) {
        HudContext ctx = new HudContext(graphics, Minecraft.getInstance().font);
        for (Module m : modules) {
            if (m.isEnabled()) {
                m.onRenderHud(ctx);
            }
        }
    }
}
