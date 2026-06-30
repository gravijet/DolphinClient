package com.dolphinclient.input;

import com.mojang.blaze3d.platform.InputConstants;
import net.fabricmc.fabric.api.client.keybinding.v1.KeyBindingHelper;
import net.minecraft.client.KeyMapping;
import org.lwjgl.glfw.GLFW;

/**
 * Tastenbelegungen des Clients. Aktuell: Menü öffnen (Standard: Rechte Umschalt).
 * Übersetzungen in {@code assets/dolphinclient/lang/en_us.json}.
 */
public final class DolphinKeybindings {
    public static KeyMapping openMenu;

    private DolphinKeybindings() {
    }

    public static void register() {
        openMenu = KeyBindingHelper.registerKeyBinding(new KeyMapping(
                "key.dolphinclient.open_menu",
                InputConstants.Type.KEYSYM,
                GLFW.GLFW_KEY_RIGHT_SHIFT,
                "category.dolphinclient"));
    }
}
