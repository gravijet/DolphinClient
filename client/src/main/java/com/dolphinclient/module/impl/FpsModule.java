package com.dolphinclient.module.impl;

import com.dolphinclient.hud.HudContext;
import com.dolphinclient.module.Module;
import net.minecraft.client.Minecraft;

/** Zeigt die aktuelle Bildrate an. */
public class FpsModule extends Module {
    public FpsModule() {
        super("fps", "FPS-Anzeige", true);
    }

    @Override
    public void onRenderHud(HudContext ctx) {
        ctx.line(Minecraft.getInstance().getFps() + " FPS");
    }
}
