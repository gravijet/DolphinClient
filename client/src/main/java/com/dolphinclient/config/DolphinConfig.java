package com.dolphinclient.config;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import net.fabricmc.loader.api.FabricLoader;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

import java.io.IOException;
import java.io.Reader;
import java.io.Writer;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.Map;

/**
 * Persistente Client-Konfiguration als JSON unter
 * {@code .minecraft/config/dolphinclient.json}.
 */
public class DolphinConfig {
    private static final Logger LOGGER = LoggerFactory.getLogger("DolphinClient/Config");
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().create();
    private static final Path PATH =
            FabricLoader.getInstance().getConfigDir().resolve("dolphinclient.json");

    /** moduleId -> aktiviert */
    public Map<String, Boolean> modules = new HashMap<>();

    public static DolphinConfig load() {
        if (Files.exists(PATH)) {
            try (Reader reader = Files.newBufferedReader(PATH)) {
                DolphinConfig cfg = GSON.fromJson(reader, DolphinConfig.class);
                if (cfg != null) {
                    return cfg;
                }
            } catch (IOException e) {
                LOGGER.warn("Konnte Config nicht laden, nutze Defaults", e);
            }
        }
        return new DolphinConfig();
    }

    public void save() {
        try {
            Files.createDirectories(PATH.getParent());
            try (Writer writer = Files.newBufferedWriter(PATH)) {
                GSON.toJson(this, writer);
            }
        } catch (IOException e) {
            LOGGER.warn("Konnte Config nicht speichern", e);
        }
    }
}
