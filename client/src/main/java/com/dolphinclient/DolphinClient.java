package com.dolphinclient;

import com.dolphinclient.config.DolphinConfig;
import com.dolphinclient.gui.DolphinMenuScreen;
import com.dolphinclient.hud.HudManager;
import com.dolphinclient.input.DolphinKeybindings;
import com.dolphinclient.module.ModuleManager;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.event.lifecycle.v1.ClientTickEvents;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Einstiegspunkt des DolphinClient-Mods (Client-seitig).
 *
 * Ziel-Minecraft: 26.1 (unobfuskiert, Mojang Official Names, Java 25).
 * Die Minecraft-berührenden Klassen (HUD, Module, GUI, Mixin) nutzen offizielle
 * Mojang-Namen — gegen die nun lesbare 26.1-Quelle verifizieren.
 */
public class DolphinClient implements ClientModInitializer {
    public static final String MOD_ID = "dolphinclient";
    public static final Logger LOGGER = LoggerFactory.getLogger("DolphinClient");

    private static DolphinConfig config;
    private static ModuleManager moduleManager;

    @Override
    public void onInitializeClient() {
        LOGGER.info("DolphinClient startet (Ziel: Minecraft 26.1)");

        config = DolphinConfig.load();
        moduleManager = new ModuleManager(config);
        moduleManager.registerDefaults();

        HudManager.init(moduleManager);
        DolphinKeybindings.register();

        ClientTickEvents.END_CLIENT_TICK.register(client -> {
            moduleManager.onTick();
            while (DolphinKeybindings.openMenu.consumeClick()) {
                client.setScreen(new DolphinMenuScreen(moduleManager));
            }
        });

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
