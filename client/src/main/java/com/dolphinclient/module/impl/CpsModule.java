package com.dolphinclient.module.impl;

import com.dolphinclient.hud.HudContext;
import com.dolphinclient.module.Module;
import com.dolphinclient.util.ClickTracker;

/** Zeigt die Klicks pro Sekunde (Linksklick) an — gespeist vom MouseHandlerMixin. */
public class CpsModule extends Module {
    public CpsModule() {
        super("cps", "CPS-Anzeige", false);
    }

    @Override
    public void onRenderHud(HudContext ctx) {
        ctx.line("CPS: " + ClickTracker.leftCps());
    }
}
