package com.dolphinclient.input;

import com.mojang.blaze3d.platform.InputConstants;
import net.fabricmc.fabric.api.client.keybinding.v1.KeyBindingHelper;
import net.minecraft.client.KeyMapping;
import org.lwjgl.glfw.GLFW;

/**
 * Tastenbelegungen des Clients:
 * - Menü öffnen (Standard: Rechte Umschalt)
 * - Zoom halten (Standard: C)
 *
 * Übersetzungen in {@code assets/dolphinclient/lang/en_us.json}.
 */
public final class DolphinKeybindings {
    public static KeyMapping openMenu;
    public static KeyMapping zoom;

    private DolphinKeybindings() {
    }

    public static void register() {
        openMenu = KeyBindingHelper.registerKeyBinding(new KeyMapping(
                "key.dolphinclient.open_menu",
                InputConstants.Type.KEYSYM,
                GLFW.GLFW_KEY_RIGHT_SHIFT,
                "category.dolphinclient"));

        zoom = KeyBindingHelper.registerKeyBinding(new KeyMapping(
                "key.dolphinclient.zoom",
                InputConstants.Type.KEYSYM,
                GLFW.GLFW_KEY_C,
                "category.dolphinclient"));
    }
}
