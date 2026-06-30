package com.dolphinclient.cosmetics;

import com.dolphinclient.DolphinClient;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.util.Optional;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;

/**
 * Holt die aktiven Cosmetics eines Spielers vom Backend.
 * Basis-URL über System-Property {@code dolphin.api} überschreibbar.
 */
public final class CosmeticsClient {
    private static final String API_BASE =
            System.getProperty("dolphin.api", "https://api.dolphinclient.example/v1");
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
                .thenApply(CosmeticsClient::parseCapeUrl)
                .exceptionally(error -> {
                    DolphinClient.LOGGER.debug("Cosmetics-Abruf fehlgeschlagen", error);
                    return Optional.empty();
                });
    }

    private static Optional<String> parseCapeUrl(HttpResponse<String> response) {
        if (response.statusCode() != 200) {
            return Optional.empty();
        }
        try {
            JsonObject obj = JsonParser.parseString(response.body()).getAsJsonObject();
            if (obj.has("cape") && obj.get("cape").isJsonObject()) {
                JsonObject cape = obj.getAsJsonObject("cape");
                if (cape.has("textureUrl")) {
                    return Optional.of(cape.get("textureUrl").getAsString());
                }
            }
        } catch (Exception e) {
            DolphinClient.LOGGER.debug("Cosmetics-Antwort nicht lesbar", e);
        }
        return Optional.empty();
    }
}
