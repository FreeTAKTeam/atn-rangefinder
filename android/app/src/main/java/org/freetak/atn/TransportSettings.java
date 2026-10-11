package org.freetak.atn;

import org.json.JSONException;
import org.json.JSONObject;

/** Required, operator-configured transport policy. No device identifiers are exported. */
final class TransportSettings {
    final int requestMtu;
    final int maxQueuedWrites;
    final int maxPendingHostRequests;
    final long connectTimeoutMs;
    final long writeTimeoutMs;
    final long hostRequestTimeoutMs;
    final long scanTimeoutMs;

    TransportSettings(JSONObject value) throws JSONException {
        requestMtu = integer(value, "requestMtu", 23, 517);
        maxQueuedWrites = integer(value, "maxQueuedWrites", 1, 4096);
        maxPendingHostRequests = integer(value, "maxPendingHostRequests", 1, 4096);
        connectTimeoutMs = integer(value, "connectTimeoutMs", 1, 3_600_000);
        writeTimeoutMs = integer(value, "writeTimeoutMs", 1, 3_600_000);
        hostRequestTimeoutMs = integer(value, "hostRequestTimeoutMs", 1, 3_600_000);
        scanTimeoutMs = integer(value, "scanTimeoutMs", 1, 3_600_000);
    }

    private static int integer(JSONObject value, String key, int min, int max) throws JSONException {
        Object input = value.get(key);
        if (!(input instanceof Number) || ((Number) input).doubleValue() != ((Number) input).intValue()) {
            throw new JSONException(key + " must be an integer");
        }
        int number = ((Number) input).intValue();
        if (number < min || number > max) throw new JSONException(key + " is out of supported bounds");
        return number;
    }
}
