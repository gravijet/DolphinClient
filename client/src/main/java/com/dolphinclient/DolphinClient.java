package com.dolphinclient;

import com.dolphinclient.config.DolphinConfig;
import com.dolphinclient.hud.HudManager;
import com.dolphinclient.module.ModuleManager;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Einstiegspunkt des DolphinClient-Mods (Client-seitig) für Minecraft 26.1.
 *
 * v0.1: Performance-HUD mit zuschaltbaren Modulen (FPS, Koordinaten, Uhrzeit,
 * Sitzungszeit, Geschwindigkeit, Keystrokes). Module werden über die Config
 * {@code .minecraft/config/dolphinclient.json} an-/ausgeschaltet. In-Game-Menü,
 * Zoom und CPS folgen, sobald die jeweiligen 26.1-APIs eingebunden sind.
 */
public class DolphinClient implements ClientModInitializer {
    public static final String MOD_ID = "dolphinclient";
    public static final Logger LOGGER = LoggerFactory.getLogger("DolphinClient");

    private static DolphinConfig config;
    private static ModuleManager moduleManager;

    @Override
    public void onInitializeClient() {
        LOGGER.info("DolphinClient startet (Minecraft 26.1)");

        config = DolphinConfig.load();
        moduleManager = new ModuleManager(config);
        moduleManager.registerDefaults();

        HudManager.init(moduleManager);

        ClientTickEvents.END_CLIENT_TICK.register(client -> moduleManager.onTick());

        LOGGER.info("DolphinClient bereit: {} Module registriert",
                moduleManager.getModules().size());
    }

    public static DolphinConfig getConfig() {
        return config;
    }

    public static ModuleManager getModuleManager() {
        return moduleManager;
    }
}
