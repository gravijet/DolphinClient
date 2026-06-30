package com.dolphinclient.module.impl;

import com.dolphinclient.hud.HudContext;
import com.dolphinclient.module.Module;

/** Zeigt die Dauer der laufenden Sitzung (reine Java-Logik). */
public class SessionTimeModule extends Module {
    private final long start = System.currentTimeMillis();

    public SessionTimeModule() {
        super("session", "Sitzungszeit", false);
    }

    @Override
    public void onRenderHud(HudContext ctx) {
        long s = (System.currentTimeMillis() - start) / 1000;
        ctx.line(String.format("Session: %02d:%02d:%02d", s / 3600, (s % 3600) / 60, s % 60));
    }
}
