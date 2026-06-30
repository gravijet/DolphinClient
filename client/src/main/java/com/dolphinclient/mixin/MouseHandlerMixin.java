package com.dolphinclient.mixin;

import com.dolphinclient.util.ClickTracker;
import net.minecraft.client.MouseHandler;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * Zählt Linksklicks für die CPS-Anzeige.
 *
 * API-Berührungspunkt (26.1, Mojang-Namen, verifizieren): Klasse
 * {@code MouseHandler}, Methode {@code onPress(long window, int button,
 * int action, int mods)}. button == 0 = links, action == 1 = gedrückt
 * (GLFW_PRESS). Falls Name/Signatur in 26.1 abweichen, hier anpassen.
 */
@Mixin(MouseHandler.class)
public class MouseHandlerMixin {
    @Inject(method = "onPress", at = @At("HEAD"))
    private void dolphin$onPress(long window, int button, int action, int mods, CallbackInfo ci) {
        if (button == 0 && action == 1) {
            ClickTracker.recordLeftClick();
        }
    }
}
