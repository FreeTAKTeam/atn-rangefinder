package org.freetak.atn;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Intent;
import android.os.Handler;
import android.os.IBinder;
import android.os.Looper;
import android.os.RemoteException;
import java.util.Arrays;
import java.util.Collections;
import java.util.HashSet;
import java.util.Set;
import java.util.UUID;
import network.reticulum.emergency.plugin.api.IRemPluginConfigurationCallback;
import network.reticulum.emergency.plugin.api.IRemPluginHost;
import network.reticulum.emergency.plugin.api.PluginProtocol;
import network.reticulum.emergency.plugin.api.RemPluginService;
import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/** Trusted REM Binder boundary; one handler owns native state, GATT, and write scheduling. */
public final class AtnPluginService extends RemPluginService implements BleTransport.Listener {
    private static final String CHANNEL = "atn-observations";
    private static final int NOTIFICATION_ID = 17012;
    private static volatile AtnPluginService instance;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private PluginPreferences preferences;
    private BleTransport ble;
    private HostRequests requests;
    private SerialWriteQueue writes;
    private TransportSettings transport;
    private IRemPluginHost host;
    private IBinder.DeathRecipient deathRecipient;
    private String connectionId = "";
    private String syncSessionId = "";
    private int apiMinor = -1;
    private String status = "Enable the plugin in REM (API 1.3), grant its map capabilities, configure transport, then select and connect a device";
    private String lastNativeResult = "";
    private boolean syncAuthorized;
    // Mirrors of native state for the UI only; Rust enforces both gates.
    private boolean colocationConfirmed;
    private boolean captureActive;
    private String captureSummary = "Capture not started";
    private boolean foreground;
    private long nativeTickBoundMs;
    private final Runnable writeTimeout = () -> {
        cancelWrites();
        ble.close("Write timed out; reconnect explicitly");
    };
    private final Runnable tick = new Runnable() {
        @Override public void run() {
            if (host == null || transport == null) return;
            if (!ble.hasPermission()) ble.close("Bluetooth permission revoked");
            call(command("tick"));
            long bound = Math.min(transport.hostRequestTimeoutMs, transport.writeTimeoutMs);
            if (nativeTickBoundMs > 0) bound = Math.min(bound, nativeTickBoundMs);
            long cadence = Math.max(1L, bound / 2);
            if (host != null && transport != null) handler.postDelayed(this, cadence);
        }
    };

    @Override public void onCreate() {
        super.onCreate();
        preferences = new PluginPreferences(this);
        ble = new BleTransport(this, handler, this);
        requests = new HostRequests(handler, envelope -> call(command("hostResponse", "envelope", envelope)));
        NotificationManager manager = getSystemService(NotificationManager.class);
        if (manager != null) manager.createNotificationChannel(new NotificationChannel(CHANNEL, "ATN observations", NotificationManager.IMPORTANCE_LOW));
        instance = this;
    }
    @Override protected Set<String> allowedHostCertificateFingerprints() {
        return new HashSet<>(Arrays.asList(BuildConfig.REM_HOST_FINGERPRINTS.split(",")));
    }
    @Override protected Set<String> allowedHostPackageNames() { return Collections.singleton(BuildConfig.REM_HOST_PACKAGE); }
    @Override protected String getDescriptorJson() {
        return "{\"pluginId\":\"org.freetak.atn\",\"apiMajor\":1,\"apiMinor\":" + PluginProtocol.MAP_SESSION_API_MINOR + "}";
    }
    @Override protected void onPluginStart(IRemPluginHost value, String sessionJson) {
        handler.post(() -> {
            endHost("binder.replaced");
            host = value;
            requests.setHost(value);
            deathRecipient = () -> handler.post(() -> endHost("binder.death"));
            try { value.asBinder().linkToDeath(deathRecipient, 0); }
            catch (RemoteException error) { endHost("binder.death"); return; }
            // REM passes the negotiated minor; a host older than 1.3 never binds this plugin.
            try { apiMinor = new JSONObject(sessionJson).optInt("apiMinor", -1); }
            catch (JSONException error) { apiMinor = -1; }
            status = apiMinor >= PluginProtocol.MAP_SESSION_API_MINOR
                ? "REM connected (plugin API 1." + apiMinor + "). Select and connect a device explicitly"
                : "REM plugin API 1." + PluginProtocol.MAP_SESSION_API_MINOR + " required; map exchange unavailable";
            try {
                transport = preferences.transport();
                JSONObject config = preferences.config();
                if (config.length() > 0) {
                    nativeTickBoundMs = Math.min(config.getLong("operationTimeoutMs"), config.getLong("queueTtlMs"));
                    call(command("configure", "config", config));
                }
            } catch (JSONException error) { status = "Transport configuration required"; }
            call(command("hostStarted", "apiMinor", Math.max(apiMinor, 0)));
            scheduleTick();
        });
    }
    @Override protected void onPluginStop(String reason) { handler.post(() -> { endHost("binder.stopped"); stopSelf(); }); }
    @Override protected void onHostEvent(String eventJson) {
        handler.post(() -> {
            try {
                PluginProtocol.requireJsonSize(eventJson, "Host event");
                call(command("hostEvent", "envelope", new JSONObject(eventJson)));
            } catch (JSONException error) { status = "Malformed host event rejected"; }
        });
    }
    @Override protected void onHostResponse(String responseJson) {
        handler.post(() -> {
            try { requests.receive(responseJson); }
            catch (JSONException error) { status = "Malformed host response rejected"; }
        });
    }
    @Override protected void onConfigurationRequest(String requestJson, IRemPluginConfigurationCallback callback) {
        handler.post(() -> {
            try {
                PluginProtocol.requireJsonSize(requestJson, "Configuration request");
                JSONObject request = new JSONObject(requestJson);
                JSONObject response;
                if ("action".equals(request.optString("type")) && "settings.open".equals(request.optString("action"))) {
                    response = new JSONObject().put("type", "actionResult").put("activity", new JSONObject().put("className", SettingsActivity.class.getName()));
                } else { response = state().put("type", "state"); }
                callback.onResponse(response.toString());
            } catch (JSONException | RemoteException error) { status = "Configuration callback rejected"; }
        });
    }

    static void ui(java.util.function.Consumer<AtnPluginService> action) {
        AtnPluginService current = instance;
        if (current != null) current.handler.post(() -> action.accept(current));
    }
    static String summary() { AtnPluginService current = instance; return current == null ? "Plugin not running. Enable it and start the node in REM" : current.status; }
    static String nativeResult() { AtnPluginService current = instance; return current == null ? "" : current.lastNativeResult; }
    static String captureStatus() { AtnPluginService current = instance; return current == null ? "Capture not started" : current.captureSummary; }
    void connectFromUi() {
        if (host == null) { status = "Start the REM node and enable this plugin first"; return; }
        try {
            transport = preferences.transport();
            scheduleTick();
            if (preferences.address().isEmpty()) { status = "Select a device first"; return; }
            disconnectFromUi();
            writes = new SerialWriteQueue(transport.maxQueuedWrites);
            connectionId = UUID.randomUUID().toString();
            ensureForeground();
            status = "Connecting selected device in read-only mode";
            ble.connect(preferences.address(), transport);
        } catch (JSONException | IllegalStateException | SecurityException error) {
            status = "Connection rejected: check permissions and transport configuration";
            ble.close(status); stopForeground();
        }
    }
    void disconnectFromUi() {
        syncAuthorized = false;
        call(command("stopSync"));
        cancelWrites();
        ble.close("Disconnected by operator");
        stopForeground();
    }
    JSONObject configureFromUi(JSONObject config) {
        syncAuthorized = false;
        cancelWrites();
        try { transport = preferences.transport(); }
        catch (JSONException error) { status = "Save valid transport policy before map configuration"; return new JSONObject(); }
        JSONObject response = call(command("configure", "config", config));
        if (response.optBoolean("ok")) {
            preferences.saveConfig(config);
            try { nativeTickBoundMs = Math.min(config.getLong("operationTimeoutMs"), config.getLong("queueTtlMs")); }
            catch (JSONException error) { nativeTickBoundMs = 0; }
        }
        scheduleTick();
        return response;
    }
    private void scheduleTick() {
        handler.removeCallbacks(tick);
        if (host != null && transport != null) handler.post(tick);
    }
    /** The operator attests that this phone, whose fix REM reports, is at the binocular. */
    void confirmColocationFromUi(String evidence) {
        if (!ble.ready() || host == null) { status = "Connect the selected device and REM first"; return; }
        JSONObject response = call(command("confirmColocation", "evidenceId", evidence));
        status = response.optBoolean("ok")
            ? "Co-location confirmed. Share points with this plugin from REM's marker sheet to display them"
            : "Co-location confirmation rejected; inspect native status";
    }
    /** Explicit, connection-scoped capture; never persisted and never restored. */
    void startCaptureFromUi() {
        if (!ble.ready() || host == null) { status = "Connect the selected device and REM first"; return; }
        if (!colocationConfirmed) { status = "Confirm the phone is at the binocular before starting capture"; return; }
        JSONObject response = call(command("startCapture"));
        JSONObject result = response.optJSONObject("result");
        if (result != null && result.optBoolean("captureStarted")) {
            status = "Capture started. Each new measurement becomes a shared REM marker; earlier measurements are not published";
        } else {
            status = "Capture not started: " + (result == null ? "native request rejected" : result.optString("reason"));
        }
    }
    void stopCaptureFromUi() {
        call(command("stopCapture"));
        status = "Capture stopped. Measurements stay raw and local";
    }
    void clearColocationFromUi() {
        syncAuthorized = false;
        syncSessionId = "";
        cancelWrites();
        call(command("clearColocation"));
        status = "Co-location confirmation withdrawn. Read-only observation remains active";
    }
    void stopSyncFromUi() {
        syncAuthorized = false;
        syncSessionId = "";
        cancelWrites();
        call(command("stopSync"));
        status = "Display session stopped. Read-only observation remains active";
    }
    void confirmFromUi(long id, String evidence) {
        JSONObject request = command("confirmDisplay", "operationId", id);
        try { request.put("evidenceId", evidence); } catch (JSONException ignored) { }
        call(request);
    }
    void iconFromUi(String pleId, byte[] png) {
        if (!syncAuthorized || !ble.ready()) { status = "Authorize a selected-point session before sending an icon"; return; }
        JSONObject request = command("setIcon", "pleId", pleId);
        try {
            request.put("png", unsigned(png));
            if (request.toString().getBytes(java.nio.charset.StandardCharsets.UTF_8).length > NativeCore.MAX_REQUEST_BYTES) {
                status = "Icon exceeds the bounded JNI envelope"; return;
            }
            call(request);
        } catch (JSONException error) { status = "Icon exceeds the bounded JNI envelope"; }
    }
    void styleFromUi(int markerType, int color) {
        if (!syncAuthorized || !ble.ready()) { status = "Authorize a selected-point session before changing style"; return; }
        JSONObject request = command("setStyle", "markerType", markerType);
        try { request.put("color", color); } catch (JSONException ignored) { }
        call(request);
    }
    private void endHost(String reason) {
        IRemPluginHost previous = host;
        host = null; syncAuthorized = false; apiMinor = -1; captureActive = false; colocationConfirmed = false;
        handler.removeCallbacks(tick);
        if (previous != null && deathRecipient != null) previous.asBinder().unlinkToDeath(deathRecipient, 0);
        deathRecipient = null;
        call(command("hostStopped"));
        requests.cancel(reason);
        cancelWrites();
        ble.close("REM host session ended");
        stopForeground();
        status = "REM host session ended; reconnect explicitly";
    }
    private JSONObject state() throws JSONException {
        return new JSONObject().put("status", status).put("deviceAlias", preferences.alias())
            .put("connected", ble.ready()).put("writeCapacity", ble.ready() ? ble.capacity() : 0)
            .put("syncAuthorized", syncAuthorized).put("syncSessionId", syncSessionId)
            .put("colocationConfirmed", colocationConfirmed).put("captureActive", captureActive).put("captureStatus", captureSummary)
            .put("apiMinor", apiMinor).put("requiredApiMinor", PluginProtocol.MAP_SESSION_API_MINOR)
            .put("nativeAvailable", NativeCore.available())
            .put("sdkCommit", BuildConfig.REM_SDK_COMMIT).put("sdkSha256", BuildConfig.REM_SDK_SHA256);
    }
    private JSONObject call(JSONObject command) {
        if (!NativeCore.available()) { status = "Native adapter unavailable in this APK"; return new JSONObject(); }
        try {
            command.put("nowMs", System.currentTimeMillis());
            JSONObject response = new JSONObject(NativeCore.dispatch(command.toString()));
            lastNativeResult = response.toString();
            mirrorCapture(response);
            if (!response.optBoolean("ok")) {
                syncAuthorized = false;
                discardWrites();
                status = response.optJSONObject("error") == null ? "Native request rejected" : response.getJSONObject("error").optString("message");
            }
            JSONArray actions = response.optJSONArray("actions");
            if (actions != null) for (int i = 0; i < actions.length(); i++) action(actions.getJSONObject(i));
            return response;
        } catch (JSONException | IllegalArgumentException | IllegalStateException error) {
            syncAuthorized = false;
            discardWrites();
            status = "Native boundary rejected malformed or unavailable action";
            return new JSONObject();
        }
    }
    /** Status-bearing responses (including every tick) carry the native capture state. */
    private void mirrorCapture(JSONObject response) {
        JSONObject result = response.optJSONObject("result");
        if (result == null) return;
        JSONObject state = result.has("capture") ? result : result.optJSONObject("status");
        JSONObject capture = state == null ? null : state.optJSONObject("capture");
        if (capture == null) return;
        colocationConfirmed = state.optBoolean("colocationConfirmed");
        captureActive = capture.optBoolean("active");
        JSONObject session = capture.optJSONObject("session");
        if (captureActive && session != null) captureSummary = "Capture active (" + session.optString("nonce") + ")";
        else if (capture.isNull("lastEnded")) captureSummary = "Capture not started";
        else captureSummary = "Capture stopped: " + capture.optString("lastEnded");
    }
    private void action(JSONObject action) throws JSONException {
        switch (action.getString("kind")) {
            case "hostRequest":
                if (transport == null) { status = "Transport configuration required for host requests"; return; }
                requests.submit(action.getJSONObject("envelope"), transport);
                break;
            case "write":
                String id = action.getString("operationId");
                int frame = action.getInt("frameNumber");
                String session = action.getString("sessionId");
                String connection = action.getString("connectionId");
                SerialWriteQueue.Write write = new SerialWriteQueue.Write(id, frame, session, connection, bytes(action.getJSONArray("bytes")));
                if (!syncAuthorized || writes == null || !ble.ready() || !syncSessionId.equals(session) || !connectionId.equals(connection)) {
                    transportResult(write, false); return;
                }
                try {
                    writes.add(write, ble.capacity());
                    pumpWrite();
                } catch (IllegalArgumentException | IllegalStateException error) {
                    transportResult(write, false);
                    cancelWrites();
                    ble.close("Write queue rejected an operation");
                }
                break;
            case "status": status = action.optString("message", action.toString()); break;
            case "sessionAuthorized":
                // REM's operator-started session authorizes writes for this connection only.
                if (ble.ready() && connectionId.equals(action.optString("connectionId"))) {
                    syncSessionId = action.getString("sessionId");
                    syncAuthorized = true;
                    status = "REM session " + syncSessionId + " authorized for display writes";
                } else { syncAuthorized = false; syncSessionId = ""; }
                break;
            case "cancelWrites": syncAuthorized = false; syncSessionId = ""; discardWrites(); break;
            case "cancelHostRequests":
                Set<String> keep = new HashSet<>();
                JSONArray ids = action.getJSONArray("keepRequestIds");
                for (int i = 0; i < ids.length(); i++) keep.add(ids.getString(i));
                requests.retain(keep);
                break;
            case "disconnect": syncAuthorized = false; syncSessionId = ""; discardWrites(); ble.close("Native session requested disconnect"); break;
            default: status = "Unsupported native action rejected";
        }
    }
    private void pumpWrite() {
        if (writes == null || writes.active() != null) return;
        SerialWriteQueue.Write active = writes.begin();
        if (active == null) return;
        if (!ble.write(active.bytes)) { writeCompleted(false); return; }
        handler.postDelayed(writeTimeout, transport.writeTimeoutMs);
    }
    @Override public void writeCompleted(boolean success) {
        handler.removeCallbacks(writeTimeout);
        SerialWriteQueue.Write active = writes == null ? null : writes.complete();
        if (active == null) return;
        transportResult(active, success);
        if (!success) { cancelWrites(); ble.close("Transport failure; reconnect explicitly"); }
        else pumpWrite();
    }
    private void transportResult(SerialWriteQueue.Write write, boolean completed) {
        long numericId;
        try { numericId = Long.parseLong(write.operationId); }
        catch (NumberFormatException error) { status = "Invalid native operation ID"; return; }
        JSONObject result = command("transportResult", "operationId", numericId);
        try {
            result.put("frameNumber", write.frameNumber).put("completed", completed)
                .put("sessionId", write.sessionId).put("connectionId", write.connectionId);
        } catch (JSONException ignored) { }
        call(result);
    }
    private void cancelWrites() {
        handler.removeCallbacks(writeTimeout);
        if (writes == null) return;
        boolean inFlight = writes.active() != null;
        java.util.List<SerialWriteQueue.Write> cancelled = writes.cancel();
        if (inFlight) ble.close("In-flight write cancelled; reconnect explicitly");
        for (SerialWriteQueue.Write write : cancelled) transportResult(write, false);
    }
    private void discardWrites() {
        handler.removeCallbacks(writeTimeout);
        if (writes == null) return;
        boolean inFlight = writes.active() != null;
        writes.cancel();
        // An in-flight GATT callback cannot be correlated after cancellation. Close it;
        // only an explicit fresh connection can authorize later writes.
        if (inFlight) ble.close("In-flight write cancelled; reconnect explicitly");
    }
    @Override public void ready(int capacity) {
        JSONObject request = command("connected", "writeCapacity", capacity);
        try { request.put("connectionId", connectionId); } catch (JSONException ignored) { }
        call(request);
        status = "Read-only notifications active; ATT write capacity " + capacity;
    }
    @Override public void notification(byte[] bytes, long timestampMs) {
        JSONObject request = command("notification", "bytes", unsigned(bytes));
        try { request.put("observedAtMs", timestampMs).put("sessionSource", connectionId); } catch (JSONException ignored) { }
        call(request);
    }
    @Override public void disconnected(String reason) {
        syncAuthorized = false;
        captureActive = false;
        call(command("disconnected", "reason", reason));
        cancelWrites();
        stopForeground();
        status = reason;
    }
    private void ensureForeground() {
        PendingIntent settings = PendingIntent.getActivity(this, 0, new Intent(this, SettingsActivity.class), PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        Notification notification = new Notification.Builder(this, CHANNEL).setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setContentTitle("ATN rangefinder connected").setContentText("Open to stop or manage selected-point synchronization")
            .setContentIntent(settings).setOngoing(true).build();
        startForeground(NOTIFICATION_ID, notification); foreground = true;
    }
    private void stopForeground() { if (foreground) { stopForeground(STOP_FOREGROUND_REMOVE); foreground = false; } }
    @Override public void onDestroy() {
        if (instance == this) instance = null;
        endHost("binder.destroyed");
        handler.removeCallbacksAndMessages(null);
        super.onDestroy();
    }
    static JSONObject command(String cmd) { return command(cmd, null, null); }
    static JSONObject command(String cmd, String key, Object value) {
        JSONObject json = new JSONObject();
        try { json.put("cmd", cmd); if (key != null) json.put(key, value); } catch (JSONException ignored) { }
        return json;
    }
    private static JSONArray unsigned(byte[] bytes) { JSONArray array = new JSONArray(); for (byte value : bytes) array.put(value & 255); return array; }
    private static byte[] bytes(JSONArray array) throws JSONException {
        byte[] bytes = new byte[array.length()];
        for (int i = 0; i < bytes.length; i++) {
            int value = array.getInt(i);
            if (value < 0 || value > 255) throw new JSONException("Byte out of range");
            bytes[i] = (byte) value;
        }
        return bytes;
    }
}
