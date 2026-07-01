package com.dolphinclient.input;

import com.mojang.blaze3d.platform.InputConstants;
import net.fabricmc.fabric.api.client.keymapping.v1.KeyMappingHelper;
import net.minecraft.client.KeyMapping;
import net.minecraft.resources.Identifier;
import org.lwjgl.glfw.GLFW;

/**
 * Tastenbelegungen (26.1): Menü öffnen (Rechte Umschalt) und Zoom halten (C).
 *
 * 26.1: Registrierung über {@link KeyMappingHelper#registerKeyMapping}; die
 * Kategorie ist jetzt ein {@link KeyMapping.Category}-Objekt (per Identifier).
 */
public final class DolphinKeybindings {
    public static KeyMapping openMenu;
    public static KeyMapping zoom;

    private DolphinKeybindings() {
    }

    public static void register() {
        KeyMapping.Category category = KeyMapping.Category.register(
                Identifier.fromNamespaceAndPath("dolphinclient", "main"));

        openMenu = KeyMappingHelper.registerKeyMapping(new KeyMapping(
                "key.dolphinclient.open_menu",
                InputConstants.Type.KEYSYM,
                GLFW.GLFW_KEY_RIGHT_SHIFT,
                category));

        zoom = KeyMappingHelper.registerKeyMapping(new KeyMapping(
                "key.dolphinclient.zoom",
                InputConstants.Type.KEYSYM,
                GLFW.GLFW_KEY_C,
                category));
    }
}
