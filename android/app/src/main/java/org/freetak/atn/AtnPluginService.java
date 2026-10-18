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
import android.util.Log;
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
    private static final String TAG = "AtnPlugin";
    private static final String CHANNEL = "atn-observations";
    private static final long CAPTURE_RETRY_MS = 5000;
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
    private int apiMinor = -1;
    private String status = "Choose your Binox and tap Connect";
    private String lastNativeResult = "";
    // REM sessions admitted for display writes on this connection (up to four).
    private final Set<String> authorizedSessions = new HashSet<>();
    private String sessionsSummary = "No display sessions";
    // Mirrors of native state for the UI only; Rust enforces both gates.
    private boolean colocationConfirmed;
    private boolean captureActive;
    private boolean hostReady;
    private int observationCount;
    private String lastMeasurement = "";
    private int displayedMarkers;
    // The operator's answer at Connect: this phone is with the binocular.
    private boolean withBinocular;
    private long lastCaptureAttemptMs;
    private String captureSummary = "Capture not started";
    private boolean foreground;
    private long nativeTickBoundMs;
    private final Runnable writeTimeout = () -> {
        cancelWrites("timeout");
        ble.close("Write timed out; reconnect explicitly");
    };
    private final Runnable tick = new Runnable() {
        @Override public void run() {
            if (host == null || transport == null) return;
            if (!ble.hasPermission()) ble.close("Bluetooth permission revoked");
            call(command("tick"));
            ensureCapture();
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
            Log.i(TAG, "REM bound, plugin API 1." + apiMinor);
            status = apiMinor >= PluginProtocol.MAP_SESSION_API_MINOR
                ? "REM connected"
                : "REM is too old: plugin API 1." + PluginProtocol.MAP_SESSION_API_MINOR + " required";
            try { applyConfig(preferences.overrides()); }
            catch (JSONException error) { status = "Saved override is invalid: " + error.getMessage(); }
            call(command("hostStarted", "apiMinor", Math.max(apiMinor, 0)));
            scheduleTick();
        });
    }
    @Override protected void onPluginStop(String reason) { handler.post(() -> { endHost("binder.stopped"); stopSelf(); }); }
    @Override protected void onHostEvent(String eventJson) {
        handler.post(() -> {
            try {
                PluginProtocol.requireJsonSize(eventJson, "Host event");
                JSONObject envelope = new JSONObject(eventJson);
                JSONObject payload = envelope.optJSONObject("payload");
                Log.i(TAG, "REM event " + envelope.optString("event") + " session=" + (payload == null ? "" : payload.optString("sessionId")));
                JSONObject response = call(command("hostEvent", "envelope", envelope));
                JSONObject result = response.optJSONObject("result");
                if (result != null && result.has("ignored")) Log.i(TAG, "REM event ignored: " + result.optString("ignored"));
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
    static String nativeResult() { AtnPluginService current = instance; return current == null ? "" : current.lastNativeResult; }
    static String sessionsStatus() { AtnPluginService current = instance; return current == null ? "No display sessions" : current.sessionsSummary; }
    /** One plain-language line: REM, Binox, measurements and what is on the Binox. */
    static String statusLine() {
        AtnPluginService current = instance;
        if (current == null) return "REM: plugin not running. Enable ATN Rangefinder in REM and start the node";
        return current.describe();
    }
    private String describe() {
        String rem = host == null ? "not running" : hostReady ? "connected" : "connecting";
        String binox = ble.ready() ? "connected" : (writes != null && !connectionId.isEmpty() ? "connecting" : "not connected");
        String measurements;
        if (!preferences.sendMeasurements()) measurements = "off";
        else if (captureActive) measurements = "on";
        else if (ble.ready() && !withBinocular) measurements = "off (phone not with the binocular)";
        else measurements = "waiting";
        StringBuilder line = new StringBuilder()
            .append("REM: ").append(rem)
            .append("\nBinox: ").append(binox)
            .append("\nMeasurements to REM: ").append(measurements)
            .append("\nOn the Binox: ").append(displayedMarkers == 0 ? "nothing shared" : displayedMarkers + " REM marker" + (displayedMarkers == 1 ? "" : "s"));
        if (!lastMeasurement.isEmpty()) line.append("\nLast measurement: ").append(lastMeasurement);
        line.append("\n\n").append(status);
        return line.toString();
    }
    void connectFromUi(boolean phoneWithBinocular) {
        if (host == null) { status = "Start the REM node and enable this plugin first"; return; }
        withBinocular = phoneWithBinocular;
        try {
            transport = preferences.transport();
            scheduleTick();
            if (preferences.address().isEmpty()) { status = "Scan and choose your Binox first"; return; }
            disconnectTransport();
            writes = new SerialWriteQueue(transport.maxQueuedWrites);
            connectionId = UUID.randomUUID().toString();
            ensureForeground();
            status = "Connecting to " + preferences.alias();
            ble.connect(preferences.address(), transport);
        } catch (JSONException | IllegalStateException | SecurityException error) {
            status = "Could not connect: check that Bluetooth is on and permissions are granted";
            ble.close(status); stopForeground();
        }
    }
    void disconnectFromUi() {
        withBinocular = false;
        disconnectTransport();
        status = "Disconnected";
    }
    private void disconnectTransport() {
        authorizedSessions.clear();
        call(command("stopSync"));
        cancelWrites("cancelled");
        ble.close("Disconnected by operator");
        stopForeground();
    }
    /** Applies built-in defaults plus the given override; returns the native response. */
    private JSONObject applyConfig(JSONObject overrides) throws JSONException {
        JSONObject response = call(command("configure", "config", overrides));
        JSONObject result = response.optJSONObject("result");
        JSONObject config = result == null ? null : result.optJSONObject("config");
        if (!response.optBoolean("ok") || config == null) return response;
        transport = new TransportSettings(config.getJSONObject("transport"));
        nativeTickBoundMs = Math.min(config.getLong("operationTimeoutMs"), config.getLong("queueTtlMs"));
        return response;
    }
    /** Advanced: saves a partial override (empty object = built-in defaults). */
    JSONObject configureFromUi(JSONObject overrides) {
        authorizedSessions.clear();
        cancelWrites("cancelled");
        JSONObject response;
        try {
            response = applyConfig(overrides);
            if (response.optBoolean("ok")) preferences.saveOverrides(overrides);
        } catch (JSONException error) { response = new JSONObject(); status = "Override rejected: " + error.getMessage(); }
        // Configuration resets co-location natively; keep the operator's Connect answer.
        if (withBinocular && ble.ready()) call(command("confirmColocation", "evidenceId", "operator confirmed at connect"));
        scheduleTick();
        return response;
    }
    void setSendMeasurementsFromUi(boolean value) {
        preferences.setSendMeasurements(value);
        if (!value && captureActive) call(command("stopCapture"));
        lastCaptureAttemptMs = 0;
        ensureCapture();
    }
    /**
     * Keeps measurements flowing to REM whenever the operator wants that and the native gates
     * allow it; capture never publishes measurements taken before it started.
     */
    private void ensureCapture() {
        if (!preferences.sendMeasurements() || captureActive || !withBinocular || !ble.ready() || !hostReady) return;
        long now = System.currentTimeMillis();
        if (now - lastCaptureAttemptMs < CAPTURE_RETRY_MS) return;
        lastCaptureAttemptMs = now;
        if (!colocationConfirmed) call(command("confirmColocation", "evidenceId", "operator confirmed at connect"));
        JSONObject result = call(command("startCapture")).optJSONObject("result");
        if (result != null && result.optBoolean("captureStarted")) {
            Log.i(TAG, "Measurements now go to REM");
            status = "Measurements now go to REM as markers";
        } else if (result != null) {
            Log.i(TAG, "Capture not started: " + result.optString("reason"));
        }
    }
    private void scheduleTick() {
        handler.removeCallbacks(tick);
        if (host != null && transport != null) handler.post(tick);
    }
    /** An empty session ID stops every display session; otherwise only that one. */
    void stopSyncFromUi(String sessionId) {
        if (sessionId.isEmpty()) {
            authorizedSessions.clear();
            cancelWrites("cancelled");
            call(command("stopSync"));
            status = "All display sessions stopped. Read-only observation remains active";
        } else {
            call(command("stopSync", "sessionId", sessionId));
            status = "Display session " + sessionId + " stopped; other sessions continue";
        }
    }
    void confirmFromUi(String sessionId, long id, String evidence) {
        JSONObject request = command("confirmDisplay", "operationId", id);
        try { request.put("sessionId", sessionId).put("evidenceId", evidence); } catch (JSONException ignored) { }
        JSONObject result = call(request).optJSONObject("result");
        if (result != null && !result.optBoolean("accepted")) status = "Display confirmation not recorded: " + result.optString("reason");
    }
    void iconFromUi(String sessionId, String pleId, byte[] png) {
        if (!authorizedSessions.contains(sessionId) || !ble.ready()) { status = "Enter an authorized REM session ID before sending an icon"; return; }
        JSONObject request = command("setIcon", "pleId", pleId);
        try {
            request.put("sessionId", sessionId).put("png", unsigned(png));
            if (request.toString().getBytes(java.nio.charset.StandardCharsets.UTF_8).length > NativeCore.MAX_REQUEST_BYTES) {
                status = "Icon exceeds the bounded JNI envelope"; return;
            }
            JSONObject result = call(request).optJSONObject("result");
            if (result != null && !result.optBoolean("accepted")) status = "Icon not queued: " + result.optString("reason");
        } catch (JSONException error) { status = "Icon exceeds the bounded JNI envelope"; }
    }
    void styleFromUi(int markerType, int color) {
        if (authorizedSessions.isEmpty() || !ble.ready()) { status = "Authorize a selected-point session before changing style"; return; }
        JSONObject request = command("setStyle", "markerType", markerType);
        try { request.put("color", color); } catch (JSONException ignored) { }
        call(request);
    }
    private void endHost(String reason) {
        IRemPluginHost previous = host;
        host = null; authorizedSessions.clear(); apiMinor = -1; captureActive = false; colocationConfirmed = false;
        handler.removeCallbacks(tick);
        if (previous != null && deathRecipient != null) previous.asBinder().unlinkToDeath(deathRecipient, 0);
        deathRecipient = null;
        call(command("hostStopped"));
        requests.cancel(reason);
        cancelWrites("cancelled");
        ble.close("REM host session ended");
        stopForeground();
        status = "REM host session ended; reconnect explicitly";
    }
    private JSONObject state() throws JSONException {
        return new JSONObject().put("status", describe()).put("deviceAlias", preferences.alias())
            .put("connected", ble.ready()).put("writeCapacity", ble.ready() ? ble.capacity() : 0)
            .put("authorizedSessions", new JSONArray(authorizedSessions)).put("sessions", sessionsSummary)
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
            mirrorStatus(response);
            if (!response.optBoolean("ok")) {
                Log.w(TAG, command.optString("cmd") + " failed: " + response.optJSONObject("error"));
                authorizedSessions.clear();
                discardWrites();
                status = response.optJSONObject("error") == null ? "Native request rejected" : response.getJSONObject("error").optString("message");
            }
            JSONArray actions = response.optJSONArray("actions");
            if (actions != null) for (int i = 0; i < actions.length(); i++) action(actions.getJSONObject(i));
            return response;
        } catch (JSONException | IllegalArgumentException | IllegalStateException error) {
            authorizedSessions.clear();
            discardWrites();
            status = "Native boundary rejected malformed or unavailable action";
            return new JSONObject();
        }
    }
    /** Status-bearing responses (including every tick) carry native capture and session state. */
    private void mirrorStatus(JSONObject response) {
        JSONObject result = response.optJSONObject("result");
        if (result == null) return;
        JSONObject state = result.has("capture") ? result : result.optJSONObject("status");
        JSONObject capture = state == null ? null : state.optJSONObject("capture");
        if (capture == null) return;
        sessionsSummary = describeSessions(state);
        colocationConfirmed = state.optBoolean("colocationConfirmed");
        hostReady = state.optBoolean("hostReady");
        JSONArray sessions = state.optJSONArray("sessions");
        displayedMarkers = 0;
        for (int i = 0; sessions != null && i < sessions.length(); i++) {
            JSONObject entry = sessions.optJSONObject(i);
            JSONArray selected = entry == null ? null : entry.optJSONArray("selected");
            displayedMarkers += selected == null ? 0 : selected.length();
        }
        JSONArray observations = state.optJSONArray("observations");
        if (observations != null && observations.length() > 0) {
            JSONObject last = observations.optJSONObject(observations.length() - 1);
            JSONObject raw = last == null ? null : last.optJSONObject("record") == null ? null : last.optJSONObject("record").optJSONObject("raw");
            JSONObject outcome = last == null ? null : last.optJSONObject("host");
            if (raw != null) {
                lastMeasurement = raw.optInt("distance") + " m, " + raw.optInt("compass") + "\u00b0 \u2014 " + describeOutcome(outcome);
            }
        }
        captureActive = capture.optBoolean("active");
        JSONObject session = capture.optJSONObject("session");
        if (captureActive && session != null) captureSummary = "Capture active (" + session.optString("nonce") + ")";
        else if (capture.isNull("lastEnded")) captureSummary = "Capture not started";
        else captureSummary = "Capture stopped: " + capture.optString("lastEnded");
    }
    private static String describeOutcome(JSONObject outcome) {
        if (outcome == null) return "";
        switch (outcome.optString("state")) {
            case "created": return "REM marker created";
            case "alreadyExisted": return "already in REM";
            case "observerRequested": case "createRequested": case "replayRequested": return "sending to REM";
            case "rejected": return "REM refused it (" + outcome.optString("code") + ")";
            case "unknown": return "REM did not answer";
            default: return "kept on this phone (" + outcome.optString("reason") + ")";
        }
    }
    /** One line per session: slots, then transport, reply and operator display confirmation. */
    private static String describeSessions(JSONObject state) {
        StringBuilder text = new StringBuilder();
        JSONArray sessions = state.optJSONArray("sessions");
        int count = sessions == null ? 0 : sessions.length();
        for (int i = 0; i < count; i++) {
            JSONObject session = sessions.optJSONObject(i);
            if (session == null) continue;
            text.append("Session ").append(session.optString("sessionId")).append(": slots");
            JSONArray selected = session.optJSONArray("selected");
            for (int j = 0; selected != null && j < selected.length(); j++) {
                JSONObject item = selected.optJSONObject(j);
                if (item != null) text.append(' ').append(item.optInt("slotId")).append('=').append(item.optString("pleId"));
            }
            text.append("; queued ").append(session.optInt("queuedOperations"));
            JSONObject active = session.optJSONObject("active");
            if (active != null) text.append("; ").append(describeReport(active));
            JSONArray completed = session.optJSONArray("completed");
            if (completed != null && completed.length() > 0) {
                text.append("; last ").append(describeReport(completed.optJSONObject(completed.length() - 1)));
            }
            text.append('\n');
        }
        if (count == 0) text.append("No display sessions\n");
        JSONArray released = state.optJSONArray("releasedSlots");
        if (released != null && released.length() > 0) {
            text.append("Released slots (device display not cleared):");
            for (int i = 0; i < released.length(); i++) {
                JSONObject slot = released.optJSONObject(i);
                if (slot != null) text.append(' ').append(slot.optInt("slotId"));
            }
        }
        return text.toString().trim();
    }
    private static String describeReport(JSONObject report) {
        if (report == null) return "";
        return "op " + report.optLong("operationId") + " " + report.optString("kind")
            + (report.optBoolean("transportComplete") ? ", transport done" : ", transport pending")
            + (report.isNull("reply") ? ", no reply" : ", reply received")
            + (report.isNull("displayConfirmation") ? ", display unconfirmed" : ", display confirmed")
            + (report.isNull("cancelled") ? "" : ", cancelled");
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
                if (!authorizedSessions.contains(session) || writes == null || !ble.ready() || !connectionId.equals(connection)) {
                    transportResult(write, false, "notAuthorized", -1); return;
                }
                try {
                    writes.add(write, ble.capacity());
                    pumpWrite();
                } catch (IllegalArgumentException | IllegalStateException error) {
                    transportResult(write, false, "queueRejected", -1);
                    cancelWrites("cancelled");
                    ble.close("Write queue rejected an operation");
                }
                break;
            case "status":
                status = action.optString("message", action.toString());
                Log.i(TAG, "Status: " + status);
                break;
            case "sessionAuthorized":
                // REM's operator-started session authorizes writes for this connection only.
                if (ble.ready() && connectionId.equals(action.optString("connectionId"))) {
                    String authorized = action.getString("sessionId");
                    authorizedSessions.add(authorized);
                    Log.i(TAG, "Showing REM session " + authorized + " on the Binox");
                    status = "REM session " + authorized + " authorized for display writes";
                }
                break;
            case "sessionEnded":
                // A frame of that session already on the radio completes and is ignored natively.
                authorizedSessions.remove(action.getString("sessionId"));
                Log.i(TAG, "REM session " + action.getString("sessionId") + " ended: " + action.optString("reason"));
                status = "REM session " + action.getString("sessionId") + " ended: " + action.optString("reason")
                    + ". Its slots are released; the device display is not cleared";
                break;
            case "cancelWrites": authorizedSessions.clear(); discardWrites(); break;
            case "cancelHostRequests":
                Set<String> keep = new HashSet<>();
                JSONArray ids = action.getJSONArray("keepRequestIds");
                for (int i = 0; i < ids.length(); i++) keep.add(ids.getString(i));
                requests.retain(keep);
                break;
            case "disconnect": authorizedSessions.clear(); discardWrites(); ble.close("Native session requested disconnect"); break;
            default: status = "Unsupported native action rejected";
        }
    }
    private void pumpWrite() {
        if (writes == null || writes.active() != null) return;
        SerialWriteQueue.Write active = writes.begin();
        if (active == null) return;
        if (!ble.write(active.bytes)) { completeWrite(false, "writeRejected", -1); return; }
        handler.postDelayed(writeTimeout, transport.writeTimeoutMs);
    }
    @Override public void writeCompleted(boolean success, int gattStatus) {
        completeWrite(success, "gattStatus", gattStatus);
    }
    private void completeWrite(boolean success, String errorCode, int gattStatus) {
        handler.removeCallbacks(writeTimeout);
        SerialWriteQueue.Write active = writes == null ? null : writes.complete();
        if (active == null) return;
        transportResult(active, success, errorCode, gattStatus);
        if (!success) { cancelWrites("cancelled"); ble.close("Transport failure; reconnect explicitly"); }
        else pumpWrite();
    }
    /** Echoes the write's identity, the write characteristic and, on failure, a typed error. */
    private void transportResult(SerialWriteQueue.Write write, boolean completed, String errorCode, int gattStatus) {
        long numericId;
        try { numericId = Long.parseLong(write.operationId); }
        catch (NumberFormatException error) { status = "Invalid native operation ID"; return; }
        JSONObject result = command("transportResult", "operationId", numericId);
        try {
            result.put("frameNumber", write.frameNumber).put("completed", completed)
                .put("sessionId", write.sessionId).put("connectionId", write.connectionId)
                .put("characteristic", BleTransport.SEND.toString());
            if (!completed) {
                JSONObject error = new JSONObject().put("code", errorCode);
                if (gattStatus >= 0) error.put("gattStatus", gattStatus);
                result.put("error", error);
            }
        } catch (JSONException ignored) { }
        call(result);
    }
    private void cancelWrites(String code) {
        handler.removeCallbacks(writeTimeout);
        if (writes == null) return;
        boolean inFlight = writes.active() != null;
        java.util.List<SerialWriteQueue.Write> cancelled = writes.cancel();
        if (inFlight) ble.close("In-flight write cancelled; reconnect explicitly");
        for (SerialWriteQueue.Write write : cancelled) transportResult(write, false, code, -1);
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
        try {
            request.put("connectionId", connectionId).put("serviceUuid", BleTransport.SERVICE.toString())
                .put("notifyCharacteristic", BleTransport.RECEIVE.toString())
                .put("writeCharacteristic", BleTransport.SEND.toString());
        } catch (JSONException ignored) { }
        call(request);
        Log.i(TAG, "Binox connected, write capacity " + capacity);
        if (withBinocular) {
            call(command("confirmColocation", "evidenceId", "operator confirmed at connect"));
            status = "Binox connected. Take a measurement to send it to REM; markers you share in REM appear on the Binox";
        } else {
            status = "Binox connected. Measurements stay on this phone because it is not with the binocular";
        }
        lastCaptureAttemptMs = 0;
        ensureCapture();
    }
    @Override public void notification(byte[] bytes, long timestampMs, java.util.UUID characteristic) {
        JSONObject request = command("notification", "bytes", unsigned(bytes));
        try {
            request.put("observedAtMs", timestampMs).put("sessionSource", connectionId)
                .put("characteristic", characteristic.toString());
        } catch (JSONException ignored) { }
        call(request);
    }
    @Override public void disconnected(String reason) {
        authorizedSessions.clear();
        captureActive = false;
        call(command("disconnected", "reason", reason));
        cancelWrites("cancelled");
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
