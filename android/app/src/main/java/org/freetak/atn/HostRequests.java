package org.freetak.atn;

import android.os.Handler;
import android.os.RemoteException;
import java.util.HashMap;
import java.util.Map;
import java.util.Set;
import network.reticulum.emergency.plugin.api.IRemPluginHost;
import network.reticulum.emergency.plugin.api.PluginProtocol;
import org.json.JSONException;
import org.json.JSONObject;

/** Bounded request correlation. Timeout, Binder failure, host rejection and process death remain distinct. */
final class HostRequests {
    interface Listener { void response(JSONObject envelope); }
    private final Handler handler;
    private final Listener listener;
    private final Map<String, Runnable> pending = new HashMap<>();
    private IRemPluginHost host;
    HostRequests(Handler handler, Listener listener) { this.handler = handler; this.listener = listener; }
    void setHost(IRemPluginHost value) { host = value; }
    void submit(JSONObject envelope, TransportSettings settings) throws JSONException {
        String raw = envelope.toString();
        JSONObject checked = PluginProtocol.requireEnvelope(raw);
        String id = checked.getString("requestId");
        if (host == null) { fail(id, "binder.unavailable", "REM host is unavailable"); return; }
        if (pending.size() >= settings.maxPendingHostRequests || pending.containsKey(id)) {
            fail(id, "binder.capacity", "Pending host request bound reached or duplicate request ID"); return;
        }
        Runnable timeout = () -> {
            pending.remove(id);
            fail(id, "binder.timeout", "REM host request timed out");
        };
        pending.put(id, timeout);
        handler.postDelayed(timeout, settings.hostRequestTimeoutMs);
        try { host.submitRequest(raw); }
        catch (RemoteException error) {
            handler.removeCallbacks(timeout); pending.remove(id);
            fail(id, "binder.exception", "REM Binder rejected the request");
        } catch (SecurityException error) {
            handler.removeCallbacks(timeout); pending.remove(id);
            fail(id, "binder.security", "REM Binder denied the request");
        }
    }
    void receive(String raw) throws JSONException {
        JSONObject envelope = PluginProtocol.requireEnvelope(raw);
        String id = envelope.getString("requestId");
        Runnable timeout = pending.remove(id);
        if (timeout == null) return;
        handler.removeCallbacks(timeout);
        listener.response(envelope);
    }
    void cancel(String code) {
        Map<String, Runnable> previous = new HashMap<>(pending);
        pending.clear();
        host = null;
        for (Map.Entry<String, Runnable> entry : previous.entrySet()) {
            handler.removeCallbacks(entry.getValue());
            fail(entry.getKey(), code, "REM host session ended");
        }
    }
    void retain(Set<String> ids) {
        for (String id : new java.util.ArrayList<>(pending.keySet())) {
            if (!ids.contains(id)) {
                Runnable timeout = pending.remove(id);
                handler.removeCallbacks(timeout);
            }
        }
    }
    private void fail(String id, String code, String message) {
        try { listener.response(new JSONObject(PluginProtocol.errorResponse(id, code, message))); }
        catch (JSONException ignored) { /* SDK error envelopes are valid JSON. */ }
    }
}
