package com.dolphinclient.module.impl;

import com.dolphinclient.hud.HudContext;
import com.dolphinclient.module.Module;

import java.time.LocalTime;
import java.time.format.DateTimeFormatter;

/** Zeigt die echte Uhrzeit an (reine Java-Logik, versionsunabhängig). */
public class ClockModule extends Module {
    private static final DateTimeFormatter FORMAT = DateTimeFormatter.ofPattern("HH:mm:ss");

    public ClockModule() {
        super("clock", "Uhrzeit", false);
    }

    @Override
    public void onRenderHud(HudContext ctx) {
        ctx.line(LocalTime.now().format(FORMAT));
    }
}
