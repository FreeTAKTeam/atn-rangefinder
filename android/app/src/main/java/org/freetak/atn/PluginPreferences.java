package org.freetak.atn;

import android.content.Context;
import android.content.SharedPreferences;
import org.json.JSONException;
import org.json.JSONObject;

/**
 * Private local settings. The configuration is the native core's built-in Binox 4K defaults plus
 * an optional partial override; nothing has to be entered for normal use.
 */
final class PluginPreferences {
    private final SharedPreferences storage;
    PluginPreferences(Context context) {
        storage = context.getSharedPreferences("atn-local", Context.MODE_PRIVATE);
        // Earlier builds required full JSON policies; the built-in defaults replace them.
        if (storage.contains("transport") || storage.contains("config")) {
            storage.edit().remove("transport").remove("config").apply();
        }
    }
    String address() { return storage.getString("selectedAddress", ""); }
    String alias() { return storage.getString("selectedAlias", "No Binox selected"); }
    void select(String address, String alias) {
        storage.edit().putString("selectedAddress", address).putString("selectedAlias", alias).apply();
    }
    boolean sendMeasurements() { return storage.getBoolean("sendMeasurements", true); }
    void setSendMeasurements(boolean value) { storage.edit().putBoolean("sendMeasurements", value).apply(); }
    JSONObject overrides() throws JSONException { return new JSONObject(storage.getString("overrides", "{}")); }
    void saveOverrides(JSONObject json) { storage.edit().putString("overrides", json.toString()).apply(); }

    /** Built-in defaults merged with the saved override, as resolved by the native core. */
    JSONObject effectiveConfig() throws JSONException {
        return resolve(overrides());
    }
    static JSONObject resolve(JSONObject overrides) throws JSONException {
        if (!NativeCore.available()) throw new JSONException("Native adapter unavailable in this APK");
        JSONObject request = new JSONObject().put("cmd", "resolveConfig").put("config", overrides);
        JSONObject response = new JSONObject(NativeCore.dispatch(request.toString()));
        if (!response.optBoolean("ok")) {
            JSONObject error = response.optJSONObject("error");
            throw new JSONException(error == null ? "Invalid override" : error.optString("message"));
        }
        return response.getJSONObject("result").getJSONObject("config");
    }
    TransportSettings transport() throws JSONException {
        return new TransportSettings(effectiveConfig().getJSONObject("transport"));
    }
}
