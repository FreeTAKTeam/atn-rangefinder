package org.freetak.atn;

import android.Manifest;
import android.annotation.SuppressLint;
import android.app.Activity;
import android.app.AlertDialog;
import android.bluetooth.BluetoothAdapter;
import android.bluetooth.BluetoothDevice;
import android.bluetooth.BluetoothManager;
import android.bluetooth.le.BluetoothLeScanner;
import android.bluetooth.le.ScanCallback;
import android.bluetooth.le.ScanFilter;
import android.bluetooth.le.ScanResult;
import android.bluetooth.le.ScanSettings;
import android.content.pm.PackageManager;
import android.content.Intent;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.ParcelUuid;
import android.text.InputType;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.CheckBox;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import network.reticulum.emergency.plugin.api.PluginProtocol;
import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

/** Explicit local device selection, permission UI, editable policy and session authorization. */
public final class SettingsActivity extends Activity {
    private static final int PERMISSIONS = 71;
    private static final int ICON = 72;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final Map<String, BluetoothDevice> devices = new LinkedHashMap<>();
    private PluginPreferences preferences;
    private LinearLayout layout;
    private LinearLayout discovered;
    private TextView status;
    private TextView alias;
    private TextView details;
    private EditText transport;
    private EditText config;
    private EditText selection;
    private EditText confirmation;
    private EditText confirmationEvidence;
    private EditText iconPle;
    private EditText markerType;
    private EditText markerColor;
    private CheckBox scanAll;
    private String requestedIconPle;
    private BluetoothLeScanner scanner;
    private int discoveryLimit;
    private final Runnable scanTimeout = this::stopScan;
    private final Runnable refresh = new Runnable() {
        @Override public void run() {
            status.setText(AtnPluginService.summary());
            details.setText(AtnPluginService.nativeResult());
            handler.postDelayed(this, 1000L);
        }
    };

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        preferences = new PluginPreferences(this);
        setTitle("ATN Rangefinder settings");
        ScrollView scroll = new ScrollView(this);
        layout = new LinearLayout(this); layout.setOrientation(LinearLayout.VERTICAL); layout.setPadding(24, 24, 24, 24);
        scroll.addView(layout); setContentView(scroll);
        text("ATN Rangefinder", 24);
        text("Enable and trust this plugin in REM 1.5.1 or later (plugin API 1.3) and grant its map capabilities. Configure policies below, choose a device, then connect. A connection starts read-only. Eligible measurements become shared REM markers; points you share with this plugin from a REM marker sheet are displayed on the device.", 16);
        status = text("", 16);
        alias = text(preferences.alias(), 16);
        button("Grant Bluetooth permissions", this::requestBluetoothPermissions);
        scanAll = new CheckBox(this); scanAll.setText(R.string.scan_all); layout.addView(scanAll);
        button("Scan and choose a device", this::startScan);
        discovered = new LinearLayout(this); discovered.setOrientation(LinearLayout.VERTICAL); layout.addView(discovered);
        button("Connect selected device (read-only)", () -> AtnPluginService.ui(AtnPluginService::connectFromUi));
        button("Disconnect", () -> AtnPluginService.ui(AtnPluginService::disconnectFromUi));
        text("Transport policy JSON", 20);
        text("Required: requestMtu, connectTimeoutMs, writeTimeoutMs, hostRequestTimeoutMs, maxPendingHostRequests, maxQueuedWrites, scanTimeoutMs. The pending-request bound also bounds the discovered-device list. Values are choices for this installation; no device policy is assumed.", 16);
        transport = editor("{}", 7);
        try { transport.setText(preferences.transportJson().toString(2)); } catch (JSONException ignored) { }
        button("Save transport policy", () -> {
            try {
                JSONObject json = object(transport);
                preferences.saveTransport(json);
                message("Transport saved. Disconnect and reconnect to apply changed transport settings.");
            } catch (JSONException error) { message(error.getMessage()); }
        });
        text("Device semantics and map policy JSON", 20);
        text("Paste an explicit configuration matching docs/configuration.md and examples/synthetic-config.json, including hostMarker (REM catalog symbol) and slotPool. Unsupported model/firmware, stale or uncertain observer fixes and unknown reference metadata block mapping and device writes. Saving changes stops any display session.", 16);
        config = editor("{}", 12);
        try { config.setText(preferences.config().toString(2)); } catch (JSONException ignored) { }
        button("Validate and save map policy", this::saveConfig);
        text("Observer co-location", 20);
        text("REM reports this phone's own position as the observer. Confirm, for this connection only, that the phone is beside the binocular; describe the evidence. Without it measurements stay raw and shared points are rejected.", 16);
        selection = editor("", 2);
        button("Confirm phone is at the binocular", () -> {
            String evidence = selection.getText().toString().trim();
            if (evidence.isEmpty() || evidence.length() > 64) { message("Describe the co-location evidence in up to 64 characters"); return; }
            AtnPluginService.ui(service -> service.confirmColocationFromUi(evidence));
        });
        button("Withdraw co-location confirmation", () -> AtnPluginService.ui(AtnPluginService::clearColocationFromUi));
        text("Display sessions start from REM: open a marker's details sheet and choose Share with ATN Rangefinder. Slots are taken from the configured slot pool in REM's selection order; a selection larger than the pool is rejected and stopped. Stop a session here or with REM's Stop control.", 16);
        button("Stop display session", () -> AtnPluginService.ui(AtnPluginService::stopSyncFromUi));
        text("Style for the authorized selected set", 20);
        text("Marker type (0 or 1) and color (0 red, 1 orange, 2 yellow, 3 green, 4 blue, 5 gray, 6 dark gray, 7 offline gray). A style change refreshes only the authorized selected set.", 16);
        markerType = editor("", 1);
        markerColor = editor("", 1);
        button("Apply selected-set style", () -> {
            try {
                int type = Integer.parseInt(markerType.getText().toString().trim());
                int color = Integer.parseInt(markerColor.getText().toString().trim());
                if (type < 0 || type > 1 || color < 0 || color > 7) { message("Enter a supported marker type and color"); return; }
                AtnPluginService.ui(service -> service.styleFromUi(type, color));
            } catch (NumberFormatException error) { message("Enter marker type and color as integers"); }
        });
        text("Optional local PNG icon", 20);
        text("Enter the object_destination_hash of a point in the active REM session. Choose a PNG file within your configured icon bounds. Position and icon are separate operations.", 16);
        iconPle = editor("", 1);
        button("Choose a PNG and send to selected PLE", () -> {
            requestedIconPle = iconPle.getText().toString().trim();
            if (requestedIconPle.isEmpty()) { message("Enter a selected PLE ID"); return; }
            startActivityForResult(new Intent(Intent.ACTION_OPEN_DOCUMENT).setType("image/png").addCategory(Intent.CATEGORY_OPENABLE), ICON);
        });
        text("Confirm device display (after looking at the device)", 20);
        text("Transport completion and reply bytes do not prove display success. Enter the operationId from native status only after observing the expected marker on the selected device.", 16);
        confirmation = editor("", 1);
        text("Describe the display evidence you observed", 16);
        confirmationEvidence = editor("", 2);
        button("Record observed display confirmation", () -> {
            try {
                long operationId = Long.parseLong(confirmation.getText().toString().trim());
                String evidence = confirmationEvidence.getText().toString().trim();
                if (operationId < 0 || evidence.isEmpty()) { message("Enter a valid operation ID and observed evidence"); return; }
                AtnPluginService.ui(service -> service.confirmFromUi(operationId, evidence));
            } catch (NumberFormatException error) { message("Operation ID must be a nonnegative integer"); }
        });
        text("Native status and local raw observation details", 20);
        details = text("", 12);
    }
    private void saveConfig() {
        try {
            JSONObject json = object(config);
            // Use the saved transport settings, so JNI and the platform share one explicit policy.
            json.put("transport", preferences.transportJson());
            new TransportSettings(json.getJSONObject("transport"));
            if (!NativeCore.available()) { message("Native adapter is missing from this APK"); return; }
            AtnPluginService.ui(service -> {
                JSONObject response = service.configureFromUi(json);
                message(response.optBoolean("ok") ? "Configuration validated and saved; synchronization stopped" : response.toString());
            });
            // Configuration may be prepared before REM starts; all JNI calls run on the main thread.
            if (AtnPluginService.summary().startsWith("Plugin not running")) {
                JSONObject response = new JSONObject(NativeCore.dispatch(AtnPluginService.command("configure", "config", json).toString()));
                if (response.optBoolean("ok")) preferences.saveConfig(json);
                message(response.toString());
            }
        } catch (JSONException error) { message(error.getMessage()); }
    }
    private JSONObject object(EditText view) throws JSONException {
        String raw = view.getText().toString();
        PluginProtocol.requireJsonSize(raw, "Configuration");
        return new JSONObject(raw);
    }
    private void requestBluetoothPermissions() {
        List<String> requested = new ArrayList<>();
        if (Build.VERSION.SDK_INT >= 31) {
            requested.add(Manifest.permission.BLUETOOTH_SCAN); requested.add(Manifest.permission.BLUETOOTH_CONNECT);
        } else { requested.add(Manifest.permission.ACCESS_FINE_LOCATION); requested.add(Manifest.permission.ACCESS_COARSE_LOCATION); }
        if (Build.VERSION.SDK_INT >= 33) requested.add(Manifest.permission.POST_NOTIFICATIONS);
        List<String> missing = new ArrayList<>();
        for (String permission : requested) if (checkSelfPermission(permission) != PackageManager.PERMISSION_GRANTED) missing.add(permission);
        if (missing.isEmpty()) { message("Permissions already granted"); return; }
        requestPermissions(missing.toArray(new String[0]), PERMISSIONS);
    }
    private boolean hasScanPermission() {
        String permission = Build.VERSION.SDK_INT >= 31 ? Manifest.permission.BLUETOOTH_SCAN : Manifest.permission.ACCESS_FINE_LOCATION;
        return checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED
            && (Build.VERSION.SDK_INT < 31 || checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) == PackageManager.PERMISSION_GRANTED);
    }
    @Override public void onRequestPermissionsResult(int requestCode, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(requestCode, permissions, results);
        if (requestCode == PERMISSIONS) message(hasScanPermission() ? "Bluetooth permissions granted; choose a device" : "Bluetooth permissions denied. Grant them in Android settings before connecting");
    }
    @Override protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != ICON || resultCode != RESULT_OK || data == null || data.getData() == null) return;
        String pleId = requestedIconPle;
        if (pleId == null || pleId.isEmpty()) { message("Icon selection context expired; choose again"); return; }
        new Thread(() -> {
            try {
                int limit = preferences.config().getJSONObject("pngLimits").getInt("maxEncodedBytes");
                // The on-wire icon length is u16; the Rust core additionally validates PNG content.
                if (limit < 1 || limit > 65_535) throw new JSONException("Configure maxEncodedBytes within the protocol's 16-bit limit");
                byte[] owned;
                try (InputStream input = getContentResolver().openInputStream(data.getData()); ByteArrayOutputStream output = new ByteArrayOutputStream()) {
                    if (input == null) throw new java.io.IOException("PNG file unavailable");
                    byte[] buffer = new byte[Math.min(limit, 4096)];
                    int count;
                    while ((count = input.read(buffer)) != -1) {
                        if (output.size() + count > limit) throw new java.io.IOException("PNG exceeds configured size bound");
                        output.write(buffer, 0, count);
                    }
                    owned = output.toByteArray();
                }
                handler.post(() -> AtnPluginService.ui(service -> service.iconFromUi(pleId, owned)));
            } catch (java.io.IOException | JSONException | SecurityException error) {
                handler.post(() -> message("PNG unavailable or outside configured bounds"));
            }
        }, "atn-local-icon").start();
    }
    @SuppressLint("MissingPermission") private void startScan() {
        stopScan();
        if (!hasScanPermission()) { requestBluetoothPermissions(); return; }
        try {
            TransportSettings settings = preferences.transport();
            discoveryLimit = settings.maxPendingHostRequests;
            BluetoothManager manager = getSystemService(BluetoothManager.class);
            BluetoothAdapter adapter = manager == null ? null : manager.getAdapter();
            if (adapter == null || !adapter.isEnabled()) { message("Enable Bluetooth in Android settings first"); return; }
            scanner = adapter.getBluetoothLeScanner();
            if (scanner == null) { message("BLE scanning is unavailable"); return; }
            devices.clear(); discovered.removeAllViews();
            ScanFilter filter = new ScanFilter.Builder().setServiceUuid(new ParcelUuid(BleTransport.SERVICE)).build();
            List<ScanFilter> filters = scanAll.isChecked() ? Collections.emptyList() : Collections.singletonList(filter);
            scanner.startScan(filters, new ScanSettings.Builder().setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY).build(), scanCallback);
            handler.postDelayed(scanTimeout, settings.scanTimeoutMs);
            message("Scanning. Tap the device you intend to use; no device is selected automatically");
        } catch (JSONException | SecurityException error) { message("Scanning requires valid transport configuration and Bluetooth permissions"); }
    }
    private final ScanCallback scanCallback = new ScanCallback() {
        @Override public void onScanResult(int callbackType, ScanResult result) {
            handler.post(() -> addDevice(result.getDevice()));
        }
        @Override public void onScanFailed(int code) { handler.post(() -> { stopScan(); message("Android BLE scanning failed: " + code); }); }
    };
    @SuppressLint("MissingPermission") private void addDevice(BluetoothDevice device) {
        if (scanner == null) return;
        try {
            String address = device.getAddress();
            if (devices.containsKey(address) || devices.size() >= discoveryLimit) return;
            devices.put(address, device);
            String name = device.getName() == null ? "ATN device" : device.getName();
            Button choice = new Button(this);
            choice.setText(getString(R.string.device_choice, name, address.substring(Math.max(0, address.length() - 5))));
            choice.setOnClickListener(view -> {
                stopScan();
                AtnPluginService.ui(AtnPluginService::disconnectFromUi);
                preferences.select(address, name);
                alias.setText(name);
                message("Device selected. Connect explicitly to observe notifications");
            });
            discovered.addView(choice);
        } catch (SecurityException error) { stopScan(); message("Bluetooth permission revoked"); }
    }
    @SuppressLint("MissingPermission") private void stopScan() {
        handler.removeCallbacks(scanTimeout);
        BluetoothLeScanner previous = scanner; scanner = null;
        if (previous != null) try { previous.stopScan(scanCallback); } catch (SecurityException ignored) { }
    }
    private TextView text(String value, int size) {
        TextView text = new TextView(this); text.setText(value); text.setTextSize(size); text.setTextIsSelectable(true);
        layout.addView(text); return text;
    }
    private EditText editor(String value, int lines) {
        EditText editor = new EditText(this); editor.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE);
        editor.setText(value); editor.setMinLines(lines); editor.setMaxLines(lines + 3);
        layout.addView(editor, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        return editor;
    }
    private void button(String label, Runnable action) {
        Button button = new Button(this); button.setText(label); button.setOnClickListener(view -> action.run()); layout.addView(button);
    }
    private void message(String value) { new AlertDialog.Builder(this).setMessage(value).setPositiveButton(android.R.string.ok, null).show(); }
    @Override protected void onResume() { super.onResume(); handler.post(refresh); }
    @Override protected void onPause() { stopScan(); handler.removeCallbacks(refresh); super.onPause(); }
    @Override protected void onDestroy() { stopScan(); handler.removeCallbacksAndMessages(null); super.onDestroy(); }
}
