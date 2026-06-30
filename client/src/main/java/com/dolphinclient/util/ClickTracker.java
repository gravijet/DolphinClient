package com.dolphinclient.util;

import java.util.ArrayDeque;
import java.util.Deque;

/**
 * Rollierende Zählung der Linksklicks der letzten Sekunde (für die CPS-Anzeige).
 *
 * Reine Logik, ohne Minecraft-Abhängigkeit. Wird vom {@code MouseHandlerMixin}
 * befüllt und von {@code CpsModule} gelesen. Nur vom Client-Thread benutzen.
 */
public final class ClickTracker {
    private static final long WINDOW_MS = 1000;
    private static final Deque<Long> leftClicks = new ArrayDeque<>();

    private ClickTracker() {
    }

    public static void recordLeftClick() {
        leftClicks.addLast(System.currentTimeMillis());
    }

    /** Klicks in der letzten Sekunde. */
    public static int leftCps() {
        prune();
        return leftClicks.size();
    }

    private static void prune() {
        long cutoff = System.currentTimeMillis() - WINDOW_MS;
        while (!leftClicks.isEmpty() && leftClicks.peekFirst() < cutoff) {
            leftClicks.pollFirst();
        }
    }
}
