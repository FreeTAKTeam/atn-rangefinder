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
import android.content.Intent;
import android.content.pm.PackageManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.ParcelUuid;
import android.text.InputType;
import android.view.View;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.CheckBox;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import network.reticulum.emergency.plugin.api.PluginProtocol;
import org.json.JSONException;
import org.json.JSONObject;

/**
 * Operator screen: pick the Binox, connect, and everything else happens on its own. Engineering
 * settings, icons and display confirmation live under Advanced.
 */
public final class SettingsActivity extends Activity {
    private static final int PERMISSIONS = 71;
    private static final int ICON = 72;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final Map<String, BluetoothDevice> devices = new LinkedHashMap<>();
    private PluginPreferences preferences;
    private LinearLayout layout;
    private LinearLayout advanced;
    private LinearLayout discovered;
    private TextView status;
    private TextView alias;
    private TextView sessions;
    private TextView details;
    private EditText overrides;
    private EditText sessionId;
    private EditText iconPle;
    private EditText markerType;
    private EditText markerColor;
    private EditText confirmation;
    private EditText confirmationEvidence;
    private CheckBox scanAll;
    private String requestedIconPle;
    private String requestedIconSession;
    private BluetoothLeScanner scanner;
    private int discoveryLimit;
    private final Runnable scanTimeout = this::stopScan;
    private final Runnable refresh = new Runnable() {
        @Override public void run() {
            status.setText(AtnPluginService.statusLine());
            sessions.setText(AtnPluginService.sessionsStatus());
            if (advanced.getVisibility() == View.VISIBLE) details.setText(AtnPluginService.nativeResult());
            handler.postDelayed(this, 1000L);
        }
    };

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        preferences = new PluginPreferences(this);
        setTitle("ATN Rangefinder");
        ScrollView scroll = new ScrollView(this);
        layout = new LinearLayout(this); layout.setOrientation(LinearLayout.VERTICAL); layout.setPadding(24, 24, 24, 24);
        scroll.addView(layout); setContentView(scroll);
        text(layout, "ATN Rangefinder " + BuildConfig.VERSION_NAME, 24);
        status = text(layout, "", 18);

        text(layout, "Binox", 20);
        alias = text(layout, preferences.alias(), 16);
        button(layout, "Scan for Binox", this::startScan);
        discovered = new LinearLayout(this); discovered.setOrientation(LinearLayout.VERTICAL); layout.addView(discovered);
        button(layout, "Connect", this::askAndConnect);
        button(layout, "Disconnect", () -> AtnPluginService.ui(AtnPluginService::disconnectFromUi));
        text(layout, "Do not pair the Binox in Android's Bluetooth settings; this screen connects to it directly.", 14);

        CheckBox send = new CheckBox(this);
        send.setText(R.string.send_measurements);
        send.setChecked(preferences.sendMeasurements());
        send.setOnCheckedChangeListener((view, checked) -> AtnPluginService.ui(service -> service.setSendMeasurementsFromUi(checked)));
        layout.addView(send);

        text(layout, "On the Binox", 20);
        text(layout, "To show a REM marker on the Binox, open the marker in REM and choose Share with ATN Rangefinder.", 14);
        sessions = text(layout, "", 14);
        button(layout, "Stop showing REM markers", () -> AtnPluginService.ui(service -> service.stopSyncFromUi("")));

        Button toggle = button(layout, "Show advanced", null);
        advanced = new LinearLayout(this); advanced.setOrientation(LinearLayout.VERTICAL); advanced.setVisibility(View.GONE);
        layout.addView(advanced);
        toggle.setOnClickListener(view -> {
            boolean show = advanced.getVisibility() != View.VISIBLE;
            advanced.setVisibility(show ? View.VISIBLE : View.GONE);
            toggle.setText(show ? "Hide advanced" : "Show advanced");
        });
        buildAdvanced();
    }

    /** The one question: is this phone with the binocular? Its position stands in for the Binox's. */
    private void askAndConnect() {
        if (AtnPluginService.statusLine().startsWith("REM: plugin not running")) {
            message("Enable ATN Rangefinder in REM's plugin settings and start the REM node first");
            return;
        }
        new AlertDialog.Builder(this)
            .setMessage("Is this phone with the binocular?\n\nREM uses the phone's position to place measured targets and to aim shared markers on the Binox.")
            .setPositiveButton("Yes", (dialog, which) -> AtnPluginService.ui(service -> service.connectFromUi(true)))
            .setNegativeButton("No", (dialog, which) -> AtnPluginService.ui(service -> service.connectFromUi(false)))
            .show();
    }

    private void buildAdvanced() {
        text(advanced, "Configuration override", 20);
        text(advanced, "Everything defaults to the Binox 4K. Enter only what should differ, as JSON, for example {\"color\":2} or {\"transport\":{\"scanTimeoutMs\":8000}}. Leave {} for the defaults.", 14);
        overrides = editor(advanced, "{}", 3);
        try { overrides.setText(preferences.overrides().toString(2)); } catch (JSONException ignored) { }
        button(advanced, "Save override", this::saveOverrides);
        button(advanced, "Show effective configuration", () -> {
            try { message(preferences.effectiveConfig().toString(2)); }
            catch (JSONException error) { message(error.getMessage()); }
        });
        button(advanced, "Reset to Binox 4K defaults", () -> { overrides.setText("{}"); saveOverrides(); });
        scanAll = new CheckBox(this); scanAll.setText(R.string.scan_all); advanced.addView(scanAll);
        button(advanced, "Grant Bluetooth permissions", this::requestBluetoothPermissions);

        text(advanced, "REM session ID (for stopping one session, icons and display confirmation)", 16);
        sessionId = editor(advanced, "", 1);
        button(advanced, "Stop this display session", () -> {
            String id = sessionId.getText().toString().trim();
            if (id.isEmpty()) { message("Enter a REM session ID; it is listed under On the Binox"); return; }
            AtnPluginService.ui(service -> service.stopSyncFromUi(id));
        });

        text(advanced, "Marker style on the Binox", 16);
        text(advanced, "Type 0 or 1; colour 0 red, 1 orange, 2 yellow, 3 green, 4 blue, 5 gray, 6 dark gray, 7 offline gray.", 14);
        markerType = editor(advanced, "", 1);
        markerColor = editor(advanced, "", 1);
        button(advanced, "Apply style", () -> {
            try {
                int type = Integer.parseInt(markerType.getText().toString().trim());
                int color = Integer.parseInt(markerColor.getText().toString().trim());
                if (type < 0 || type > 1 || color < 0 || color > 7) { message("Enter a supported marker type and colour"); return; }
                AtnPluginService.ui(service -> service.styleFromUi(type, color));
            } catch (NumberFormatException error) { message("Enter marker type and colour as numbers"); }
        });

        text(advanced, "Icon for a shared marker (40×40 PNG)", 16);
        text(advanced, "Enter the REM session ID above and the marker's object_destination_hash.", 14);
        iconPle = editor(advanced, "", 1);
        button(advanced, "Choose PNG and send", () -> {
            requestedIconPle = iconPle.getText().toString().trim();
            requestedIconSession = sessionId.getText().toString().trim();
            if (requestedIconPle.isEmpty() || requestedIconSession.isEmpty()) { message("Enter the REM session ID and the marker ID"); return; }
            startActivityForResult(new Intent(Intent.ACTION_OPEN_DOCUMENT).setType("image/png").addCategory(Intent.CATEGORY_OPENABLE), ICON);
        });

        text(advanced, "Record that the Binox showed a marker", 16);
        text(advanced, "Delivery to the Binox does not prove it is displayed. After looking at the Binox, enter the REM session ID above, the operation ID and what you saw.", 14);
        confirmation = editor(advanced, "", 1);
        confirmationEvidence = editor(advanced, "", 2);
        button(advanced, "Record display confirmation", () -> {
            try {
                long operationId = Long.parseLong(confirmation.getText().toString().trim());
                String evidence = confirmationEvidence.getText().toString().trim();
                String session = sessionId.getText().toString().trim();
                if (operationId < 0 || evidence.isEmpty() || session.isEmpty()) { message("Enter session ID, operation ID and what you saw"); return; }
                AtnPluginService.ui(service -> service.confirmFromUi(session, operationId, evidence));
            } catch (NumberFormatException error) { message("Operation ID must be a number"); }
        });

        text(advanced, "Native status", 16);
        details = text(advanced, "", 12);
    }

    private void saveOverrides() {
        try {
            String raw = overrides.getText().toString().trim();
            PluginProtocol.requireJsonSize(raw.isEmpty() ? "{}" : raw, "Override");
            JSONObject json = new JSONObject(raw.isEmpty() ? "{}" : raw);
            PluginPreferences.resolve(json); // validates against the defaults before saving
            if (AtnPluginService.statusLine().startsWith("REM: plugin not running")) {
                preferences.saveOverrides(json);
                message("Override saved; it applies when REM starts the plugin");
                return;
            }
            AtnPluginService.ui(service -> {
                JSONObject response = service.configureFromUi(json);
                message(response.optBoolean("ok") ? "Override saved" : "Override rejected: " + response.optJSONObject("error"));
            });
        } catch (JSONException error) { message("Override rejected: " + error.getMessage()); }
    }

    private void requestBluetoothPermissions() {
        List<String> missing = missingPermissions();
        if (missing.isEmpty()) { message("Bluetooth permissions already granted"); return; }
        requestPermissions(missing.toArray(new String[0]), PERMISSIONS);
    }
    private List<String> missingPermissions() {
        List<String> requested = new ArrayList<>();
        if (Build.VERSION.SDK_INT >= 31) {
            requested.add(Manifest.permission.BLUETOOTH_SCAN); requested.add(Manifest.permission.BLUETOOTH_CONNECT);
        } else { requested.add(Manifest.permission.ACCESS_FINE_LOCATION); requested.add(Manifest.permission.ACCESS_COARSE_LOCATION); }
        if (Build.VERSION.SDK_INT >= 33) requested.add(Manifest.permission.POST_NOTIFICATIONS);
        List<String> missing = new ArrayList<>();
        for (String permission : requested) if (checkSelfPermission(permission) != PackageManager.PERMISSION_GRANTED) missing.add(permission);
        return missing;
    }
    private boolean hasScanPermission() {
        String permission = Build.VERSION.SDK_INT >= 31 ? Manifest.permission.BLUETOOTH_SCAN : Manifest.permission.ACCESS_FINE_LOCATION;
        return checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED
            && (Build.VERSION.SDK_INT < 31 || checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT) == PackageManager.PERMISSION_GRANTED);
    }
    @Override public void onRequestPermissionsResult(int requestCode, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(requestCode, permissions, results);
        if (requestCode != PERMISSIONS) return;
        if (hasScanPermission()) startScan();
        else message("Bluetooth permission was denied. Allow Nearby devices for ATN Rangefinder in Android settings");
    }
    @Override protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != ICON || resultCode != RESULT_OK || data == null || data.getData() == null) return;
        String pleId = requestedIconPle;
        String session = requestedIconSession;
        if (pleId == null || pleId.isEmpty() || session == null || session.isEmpty()) { message("Icon selection expired; choose again"); return; }
        new Thread(() -> {
            try {
                int limit = preferences.effectiveConfig().getJSONObject("pngLimits").getInt("maxEncodedBytes");
                // The on-wire icon length is u16; the Rust core additionally validates PNG content.
                if (limit < 1 || limit > 65_535) throw new JSONException("Icon size limit must fit the protocol's 16-bit length");
                byte[] owned;
                try (InputStream input = getContentResolver().openInputStream(data.getData()); ByteArrayOutputStream output = new ByteArrayOutputStream()) {
                    if (input == null) throw new java.io.IOException("PNG file unavailable");
                    byte[] buffer = new byte[Math.min(limit, 4096)];
                    int count;
                    while ((count = input.read(buffer)) != -1) {
                        if (output.size() + count > limit) throw new java.io.IOException("PNG exceeds the size limit");
                        output.write(buffer, 0, count);
                    }
                    owned = output.toByteArray();
                }
                handler.post(() -> AtnPluginService.ui(service -> service.iconFromUi(session, pleId, owned)));
            } catch (java.io.IOException | JSONException | SecurityException error) {
                handler.post(() -> message("PNG unavailable or too large"));
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
            if (adapter == null || !adapter.isEnabled()) { message("Turn Bluetooth on in Android settings first"); return; }
            scanner = adapter.getBluetoothLeScanner();
            if (scanner == null) { message("Bluetooth scanning is unavailable on this phone"); return; }
            devices.clear(); discovered.removeAllViews();
            ScanFilter filter = new ScanFilter.Builder().setServiceUuid(new ParcelUuid(BleTransport.SERVICE)).build();
            List<ScanFilter> filters = scanAll.isChecked() ? Collections.emptyList() : Collections.singletonList(filter);
            scanner.startScan(filters, new ScanSettings.Builder().setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY).build(), scanCallback);
            handler.postDelayed(scanTimeout, settings.scanTimeoutMs);
            alias.setText(R.string.scanning);
        } catch (JSONException error) {
            message("Could not read the Bluetooth settings: " + error.getMessage());
        } catch (SecurityException error) {
            message("Bluetooth permission is missing; tap Scan for Binox again to grant it");
        }
    }
    private final ScanCallback scanCallback = new ScanCallback() {
        @Override public void onScanResult(int callbackType, ScanResult result) {
            handler.post(() -> addDevice(result.getDevice()));
        }
        @Override public void onScanFailed(int code) { handler.post(() -> { stopScan(); message("Bluetooth scan failed (code " + code + ")"); }); }
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
                discovered.removeAllViews();
                askAndConnect();
            });
            discovered.addView(choice);
        } catch (SecurityException error) { stopScan(); message("Bluetooth permission revoked"); }
    }
    @SuppressLint("MissingPermission") private void stopScan() {
        handler.removeCallbacks(scanTimeout);
        BluetoothLeScanner previous = scanner; scanner = null;
        if (previous != null) try { previous.stopScan(scanCallback); } catch (SecurityException ignored) { }
        if (alias != null && getString(R.string.scanning).contentEquals(alias.getText())) alias.setText(preferences.alias());
    }
    private TextView text(LinearLayout parent, String value, int size) {
        TextView text = new TextView(this); text.setText(value); text.setTextSize(size); text.setTextIsSelectable(true);
        parent.addView(text); return text;
    }
    private EditText editor(LinearLayout parent, String value, int lines) {
        EditText editor = new EditText(this); editor.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE);
        editor.setText(value); editor.setMinLines(lines); editor.setMaxLines(lines + 3);
        parent.addView(editor, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
        return editor;
    }
    private Button button(LinearLayout parent, String label, Runnable action) {
        Button button = new Button(this); button.setText(label);
        if (action != null) button.setOnClickListener(view -> action.run());
        parent.addView(button); return button;
    }
    private void message(String value) { new AlertDialog.Builder(this).setMessage(value).setPositiveButton(android.R.string.ok, null).show(); }
    @Override protected void onResume() { super.onResume(); handler.post(refresh); }
    @Override protected void onPause() { stopScan(); handler.removeCallbacks(refresh); super.onPause(); }
    @Override protected void onDestroy() { stopScan(); handler.removeCallbacksAndMessages(null); super.onDestroy(); }
}
