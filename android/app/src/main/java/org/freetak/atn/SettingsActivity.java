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
    private static final int LOCATION = 73;
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
        getWindow().getDecorView().setBackgroundColor(RemStyle.SURFACE);
        ScrollView scroll = new ScrollView(this);
        scroll.setBackgroundColor(RemStyle.SURFACE);
        layout = new LinearLayout(this); layout.setOrientation(LinearLayout.VERTICAL);
        int gutter = RemStyle.dp(this, 16);
        layout.setPadding(gutter, gutter, gutter, RemStyle.dp(this, 32));
        scroll.addView(layout); setContentView(scroll);
        // Android 15 draws edge to edge: keep content clear of the status and gesture bars.
        scroll.setOnApplyWindowInsetsListener((view, insets) -> {
            view.setPadding(0, insets.getSystemWindowInsetTop(), 0, insets.getSystemWindowInsetBottom());
            return insets;
        });

        TextView title = new TextView(this); title.setText(R.string.app_title); RemStyle.title(title); layout.addView(title);
        text(layout, "Binox 4K \u00b7 REM plugin " + BuildConfig.VERSION_NAME, 14);

        LinearLayout statusPanel = panel(layout);
        text(statusPanel, "Status", 20);
        status = text(statusPanel, "", 16);

        LinearLayout binox = panel(layout);
        text(binox, "Binox", 20);
        alias = text(binox, preferences.alias(), 18);
        button(binox, "Scan for Binox", this::startScan, false);
        discovered = new LinearLayout(this); discovered.setOrientation(LinearLayout.VERTICAL); binox.addView(discovered);
        button(binox, "Connect", this::askAndConnect, false);
        button(binox, "Disconnect", () -> AtnPluginService.ui(AtnPluginService::disconnectFromUi), true);
        text(binox, "Do not pair the Binox in Android's Bluetooth settings; this screen connects to it directly.", 14);

        LinearLayout measurements = panel(layout);
        text(measurements, "Measurements", 20);
        CheckBox send = new CheckBox(this);
        send.setText(R.string.send_measurements);
        RemStyle.checkbox(send);
        send.setChecked(preferences.sendMeasurements());
        send.setOnCheckedChangeListener((view, checked) -> AtnPluginService.ui(service -> service.setSendMeasurementsFromUi(checked)));
        measurements.addView(send);

        LinearLayout display = panel(layout);
        text(display, "On the Binox", 20);
        text(display, "REM markers within the Binox's range appear on it automatically, nearest first, in NATO colours. Once, in REM: Settings \u2192 Plugins \u2192 ATN Rangefinder \u2192 allow \"Read all map markers near this device\".", 14);
        sessions = text(display, "", 14);
        button(display, "Stop showing REM markers", () -> AtnPluginService.ui(service -> service.stopSyncFromUi("")), true);

        Button toggle = button(layout, "Show advanced", null, false);
        advanced = panel(layout); advanced.setVisibility(View.GONE);
        toggle.setOnClickListener(view -> {
            boolean show = advanced.getVisibility() != View.VISIBLE;
            advanced.setVisibility(show ? View.VISIBLE : View.GONE);
            toggle.setText(show ? "Hide advanced" : "Show advanced");
        });
        buildAdvanced();
    }

    /** A REM-style card holding one section. */
    private LinearLayout panel(LinearLayout parent) {
        LinearLayout card = new LinearLayout(this);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setBackground(RemStyle.panel(this));
        int padding = RemStyle.dp(this, 14);
        card.setPadding(padding, padding, padding, padding);
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        params.topMargin = RemStyle.dp(this, 12);
        parent.addView(card, params);
        return card;
    }

    /** The one question: is this phone with the binocular? Its position stands in for the Binox's. */
    private void askAndConnect() {
        if (AtnPluginService.statusLine().startsWith("REM: plugin not running")) {
            message("Enable ATN Rangefinder in REM's plugin settings and start the REM node first");
            return;
        }
        dialog()
            .setMessage("Is this phone with the binocular?\n\nThe phone's GPS position is used to place measured targets and to aim REM markers on the Binox.")
            .setPositiveButton("Yes", (dialog, which) -> connectWithBinocular())
            .setNegativeButton("No", (dialog, which) -> AtnPluginService.ui(service -> service.connectFromUi(false)))
            .show();
    }

    /** The phone's GPS stands in for the Binox's position, so ask for location once. */
    private void connectWithBinocular() {
        if (checkSelfPermission(Manifest.permission.ACCESS_FINE_LOCATION) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(new String[] {Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION}, LOCATION);
            return;
        }
        AtnPluginService.ui(service -> service.connectFromUi(true));
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
        scanAll = new CheckBox(this); scanAll.setText(R.string.scan_all); RemStyle.checkbox(scanAll); advanced.addView(scanAll);
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
        if (requestCode == LOCATION) {
            // Without it the plugin falls back to REM's position.
            AtnPluginService.ui(service -> service.connectFromUi(true));
            return;
        }
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
            RemStyle.button(choice, false);
            choice.setOnClickListener(view -> {
                stopScan();
                AtnPluginService.ui(AtnPluginService::disconnectFromUi);
                preferences.select(address, name);
                alias.setText(name);
                discovered.removeAllViews();
                askAndConnect();
            });
            LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
            params.topMargin = RemStyle.dp(this, 8);
            discovered.addView(choice, params);
        } catch (SecurityException error) { stopScan(); message("Bluetooth permission revoked"); }
    }
    @SuppressLint("MissingPermission") private void stopScan() {
        handler.removeCallbacks(scanTimeout);
        BluetoothLeScanner previous = scanner; scanner = null;
        if (previous != null) try { previous.stopScan(scanCallback); } catch (SecurityException ignored) { }
        if (alias != null && getString(R.string.scanning).contentEquals(alias.getText())) alias.setText(preferences.alias());
    }
    /** Size 20 is a section heading; 16 and up primary text; smaller is secondary text. */
    private TextView text(LinearLayout parent, String value, int size) {
        TextView text = new TextView(this); text.setText(value); text.setTextIsSelectable(true);
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        if (size >= 20) {
            RemStyle.heading(text);
            // Selectable text ignores all-caps, so uppercase headings directly.
            text.setText(value.toUpperCase(java.util.Locale.ROOT));
            params.topMargin = parent.getChildCount() == 0 ? 0 : RemStyle.dp(this, 12);
            params.bottomMargin = RemStyle.dp(this, 4);
        } else {
            text.setTextSize(size);
            RemStyle.body(text, size < 16);
            params.topMargin = RemStyle.dp(this, 4);
        }
        parent.addView(text, params); return text;
    }
    private EditText editor(LinearLayout parent, String value, int lines) {
        EditText editor = new EditText(this); editor.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE);
        editor.setText(value); editor.setMinLines(lines); editor.setMaxLines(lines + 3);
        RemStyle.input(editor);
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        params.topMargin = RemStyle.dp(this, 6);
        parent.addView(editor, params);
        return editor;
    }
    private Button button(LinearLayout parent, String label, Runnable action) { return button(parent, label, action, false); }
    private Button button(LinearLayout parent, String label, Runnable action, boolean danger) {
        Button button = new Button(this); button.setText(label);
        RemStyle.button(button, danger);
        if (action != null) button.setOnClickListener(view -> action.run());
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        params.topMargin = RemStyle.dp(this, 8);
        parent.addView(button, params); return button;
    }
    private AlertDialog.Builder dialog() { return new AlertDialog.Builder(this, android.R.style.Theme_Material_Dialog_Alert); }
    private void message(String value) { dialog().setMessage(value).setPositiveButton(android.R.string.ok, null).show(); }
    @Override protected void onResume() { super.onResume(); handler.post(refresh); }
    @Override protected void onPause() { stopScan(); handler.removeCallbacks(refresh); super.onPause(); }
    @Override protected void onDestroy() { stopScan(); handler.removeCallbacksAndMessages(null); super.onDestroy(); }
}
