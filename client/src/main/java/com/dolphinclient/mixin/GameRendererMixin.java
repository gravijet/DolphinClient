package com.dolphinclient.mixin;

import com.dolphinclient.module.impl.ZoomModule;
import com.llamalad7.mixinextras.injector.ModifyReturnValue;
import net.minecraft.client.renderer.GameRenderer;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;

/**
 * Skaliert das Sichtfeld für den Zoom.
 *
 * API-Berührungspunkt (26.1, Mojang-Namen, verifizieren): Klasse
 * {@code GameRenderer}, Methode {@code getFov(...)} mit Rückgabetyp
 * {@code double}. {@code @ModifyReturnValue} (MixinExtras, mit Fabric Loader
 * gebündelt) multipliziert den Rückgabewert, wenn der Zoom aktiv ist.
 */
@Mixin(GameRenderer.class)
public class GameRendererMixin {
    @ModifyReturnValue(method = "getFov", at = @At("RETURN"))
    private double dolphin$applyZoom(double fov) {
        return ZoomModule.isActive() ? fov * ZoomModule.factor() : fov;
    }
}
