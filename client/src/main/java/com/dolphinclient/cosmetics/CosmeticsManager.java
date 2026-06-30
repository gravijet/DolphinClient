package com.dolphinclient.cosmetics;

import java.util.Map;
import java.util.Optional;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Cache der Cape-URLs pro Spieler — fragt jede UUID höchstens einmal ab.
 *
 * Render-Integration (M5, noch offen, klar markiert): Die eigentliche
 * Cape-Darstellung erfordert (a) das Laden der Textur von der URL in eine
 * dynamische Minecraft-Textur (ResourceLocation) und (b) das Einklinken in den
 * Spieler-/Cape-Render-Layer von 26.1 (eigener RenderLayer oder Mixin). Dieser
 * versions-spezifische Renderteil ist bewusst noch nicht implementiert.
 */
public final class CosmeticsManager {
    private static final Map<UUID, Optional<String>> CACHE = new ConcurrentHashMap<>();

    private CosmeticsManager() {
    }

    /** Gecachte Cape-URL; stößt beim ersten Aufruf den asynchronen Abruf an. */
    public static Optional<String> capeUrl(UUID uuid) {
        Optional<String> cached = CACHE.get(uuid);
        if (cached != null) {
            return cached;
        }
        // Sofort als "in Arbeit" markieren, damit nicht mehrfach abgefragt wird.
        CACHE.put(uuid, Optional.empty());
        CosmeticsClient.fetchCapeUrl(uuid).thenAccept(url -> CACHE.put(uuid, url));
        return Optional.empty();
    }

    public static void invalidate(UUID uuid) {
        CACHE.remove(uuid);
    }
}
