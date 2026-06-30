package com.dolphinclient.cosmetics;

import com.dolphinclient.DolphinClient;

import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.util.Optional;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;

/**
 * Phase 3 (M5): lädt die aktiven Cosmetics eines Spielers vom Backend.
 *
 * Skelett — das Parsen der Antwort und das Rendern der Capes folgt mit dem
 * Cosmetics-System. Die Basis-URL kommt später aus der Konfiguration.
 */
public final class CosmeticsClient {
    private static final String API_BASE = "https://api.dolphinclient.example/v1";
    private static final HttpClient HTTP = HttpClient.newHttpClient();

    private CosmeticsClient() {
    }

    /** Liefert die Cape-Textur-URL eines Spielers, falls vorhanden. */
    public static CompletableFuture<Optional<String>> fetchCapeUrl(UUID uuid) {
        HttpRequest request = HttpRequest.newBuilder()
                .uri(URI.create(API_BASE + "/cosmetics/" + uuid))
                .GET()
                .build();

        return HTTP.sendAsync(request, HttpResponse.BodyHandlers.ofString())
                .thenApply(response -> {
                    if (response.statusCode() != 200) {
                        return Optional.<String>empty();
                    }
                    // TODO(M5): JSON parsen und Cape-URL extrahieren.
                    return Optional.<String>empty();
                })
                .exceptionally(error -> {
                    DolphinClient.LOGGER.debug("Cosmetics-Abruf fehlgeschlagen", error);
                    return Optional.empty();
                });
    }
}
