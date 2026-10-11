package org.freetak.atn;

import android.content.Context;
import android.content.SharedPreferences;
import org.json.JSONException;
import org.json.JSONObject;

final class PluginPreferences {
    private final SharedPreferences storage;
    PluginPreferences(Context context) { storage = context.getSharedPreferences("atn-local", Context.MODE_PRIVATE); }
    String address() { return storage.getString("selectedAddress", ""); }
    String alias() { return storage.getString("selectedAlias", "No device selected"); }
    void select(String address, String alias) {
        storage.edit().putString("selectedAddress", address).putString("selectedAlias", alias).apply();
    }
    JSONObject transportJson() throws JSONException { return new JSONObject(storage.getString("transport", "{}")); }
    TransportSettings transport() throws JSONException { return new TransportSettings(transportJson()); }
    void saveTransport(JSONObject json) throws JSONException {
        new TransportSettings(json);
        storage.edit().putString("transport", json.toString()).apply();
    }
    JSONObject config() throws JSONException { return new JSONObject(storage.getString("config", "{}")); }
    void saveConfig(JSONObject json) { storage.edit().putString("config", json.toString()).apply(); }
}
