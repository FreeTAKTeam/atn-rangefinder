package org.freetak.atn;

import android.Manifest;
import android.annotation.SuppressLint;
import android.bluetooth.BluetoothAdapter;
import android.bluetooth.BluetoothDevice;
import android.bluetooth.BluetoothGatt;
import android.bluetooth.BluetoothGattCallback;
import android.bluetooth.BluetoothGattCharacteristic;
import android.bluetooth.BluetoothGattDescriptor;
import android.bluetooth.BluetoothGattService;
import android.bluetooth.BluetoothManager;
import android.bluetooth.BluetoothProfile;
import android.bluetooth.BluetoothStatusCodes;
import android.content.Context;
import android.content.pm.PackageManager;
import android.os.Build;
import android.os.Handler;
import java.util.UUID;

/** GATT lifecycle only. Discovered UUIDs replace capture-specific handles. Never reconnects. */
@SuppressLint("MissingPermission")
final class BleTransport {
    static final UUID SERVICE = UUID.fromString("d973f2e0-b19e-11e2-9e96-0800200c9a66");
    static final UUID RECEIVE = UUID.fromString("d973f2e1-b19e-11e2-9e96-0800200c9a66");
    static final UUID SEND = UUID.fromString("d973f2e2-b19e-11e2-9e96-0800200c9a66");
    private static final UUID CCCD = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb");
    interface Listener {
        void ready(int capacity);
        void notification(byte[] bytes, long timestampMs);
        void writeCompleted(boolean success);
        void disconnected(String reason);
    }
    private final Context context;
    private final Handler handler;
    private final Listener listener;
    private BluetoothGatt gatt;
    private BluetoothGattCharacteristic writeCharacteristic;
    private boolean awaitingMtu;
    private boolean subscribed;
    private int mtu = 23;
    private int requestMtu;
    private final Runnable setupTimeout = () -> close("Connection setup timed out");

    BleTransport(Context context, Handler handler, Listener listener) {
        this.context = context; this.handler = handler; this.listener = listener;
    }
    boolean hasPermission() {
        return Build.VERSION.SDK_INT < 31 || context.checkSelfPermission(Manifest.permission.BLUETOOTH_CONNECT)
            == PackageManager.PERMISSION_GRANTED;
    }
    void connect(String address, TransportSettings settings) {
        close("New explicit connection");
        if (!hasPermission()) { listener.disconnected("Bluetooth permission required"); return; }
        BluetoothManager manager = context.getSystemService(BluetoothManager.class);
        BluetoothAdapter adapter = manager == null ? null : manager.getAdapter();
        if (adapter == null || !adapter.isEnabled()) { listener.disconnected("Bluetooth unavailable"); return; }
        mtu = 23;
        requestMtu = settings.requestMtu;
        try {
            gatt = adapter.getRemoteDevice(address).connectGatt(context, false, callback, BluetoothDevice.TRANSPORT_LE);
            if (gatt == null) { listener.disconnected("Connection unavailable"); return; }
            handler.postDelayed(setupTimeout, settings.connectTimeoutMs);
        } catch (SecurityException | IllegalArgumentException error) { close("Connection rejected"); }
    }
    int capacity() { return Math.min(mtu - 3, 512); }
    boolean ready() { return subscribed && gatt != null && writeCharacteristic != null; }
    boolean write(byte[] bytes) {
        if (!ready() || !hasPermission() || bytes.length == 0 || bytes.length > capacity()) return false;
        try {
            if (Build.VERSION.SDK_INT >= 33) {
                return gatt.writeCharacteristic(writeCharacteristic, bytes,
                    BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE) == BluetoothStatusCodes.SUCCESS;
            }
            writeCharacteristic.setWriteType(BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE);
            writeCharacteristic.setValue(bytes);
            return gatt.writeCharacteristic(writeCharacteristic);
        } catch (SecurityException error) { return false; }
    }
    void close(String reason) {
        handler.removeCallbacks(setupTimeout);
        BluetoothGatt previous = gatt;
        gatt = null; writeCharacteristic = null; subscribed = false; awaitingMtu = false;
        if (previous != null) {
            try { previous.disconnect(); } catch (SecurityException ignored) { }
            try { previous.close(); } catch (SecurityException ignored) { }
            listener.disconnected(reason);
        }
    }
    private void discover(BluetoothGatt value) {
        awaitingMtu = false;
        try { if (!value.discoverServices()) close("Service discovery rejected"); }
        catch (SecurityException error) { close("Bluetooth permission revoked"); }
    }
    private final BluetoothGattCallback callback = new BluetoothGattCallback() {
        @Override public void onConnectionStateChange(BluetoothGatt value, int status, int state) {
            handler.post(() -> {
                if (value != gatt) return;
                if (status != BluetoothGatt.GATT_SUCCESS || state == BluetoothProfile.STATE_DISCONNECTED) {
                    close("Device disconnected"); return;
                }
                if (state != BluetoothProfile.STATE_CONNECTED) return;
                try {
                    awaitingMtu = value.requestMtu(requestMtu);
                    if (!awaitingMtu) discover(value);
                } catch (SecurityException error) { close("Bluetooth permission revoked"); }
            });
        }
        @Override public void onMtuChanged(BluetoothGatt value, int negotiated, int status) {
            handler.post(() -> {
                if (value != gatt || !awaitingMtu) return;
                if (status == BluetoothGatt.GATT_SUCCESS && negotiated >= 23 && negotiated <= 517) mtu = negotiated;
                discover(value);
            });
        }
        @Override public void onServicesDiscovered(BluetoothGatt value, int status) {
            handler.post(() -> {
                if (value != gatt) return;
                if (status != BluetoothGatt.GATT_SUCCESS) { close("Service discovery failed"); return; }
                BluetoothGattService service = value.getService(SERVICE);
                BluetoothGattCharacteristic receive = service == null ? null : service.getCharacteristic(RECEIVE);
                writeCharacteristic = service == null ? null : service.getCharacteristic(SEND);
                BluetoothGattDescriptor descriptor = receive == null ? null : receive.getDescriptor(CCCD);
                if (receive == null || writeCharacteristic == null || descriptor == null
                    || (receive.getProperties() & BluetoothGattCharacteristic.PROPERTY_NOTIFY) == 0
                    || (writeCharacteristic.getProperties() & BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE) == 0) {
                    close("Required ATN characteristics unavailable"); return;
                }
                try {
                    if (!value.setCharacteristicNotification(receive, true)) { close("Notification registration rejected"); return; }
                    boolean accepted;
                    if (Build.VERSION.SDK_INT >= 33) {
                        accepted = value.writeDescriptor(descriptor, BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE) == BluetoothStatusCodes.SUCCESS;
                    } else {
                        descriptor.setValue(BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE);
                        accepted = value.writeDescriptor(descriptor);
                    }
                    if (!accepted) close("Subscription rejected");
                } catch (SecurityException error) { close("Bluetooth permission revoked"); }
            });
        }
        @Override public void onDescriptorWrite(BluetoothGatt value, BluetoothGattDescriptor descriptor, int status) {
            handler.post(() -> {
                if (value != gatt || !CCCD.equals(descriptor.getUuid())) return;
                if (status != BluetoothGatt.GATT_SUCCESS) { close("Subscription failed"); return; }
                subscribed = true;
                handler.removeCallbacks(setupTimeout);
                listener.ready(capacity());
            });
        }
        @Override public void onCharacteristicChanged(BluetoothGatt value, BluetoothGattCharacteristic characteristic) {
            received(value, characteristic, characteristic.getValue());
        }
        @Override public void onCharacteristicChanged(BluetoothGatt value, BluetoothGattCharacteristic characteristic, byte[] bytes) {
            received(value, characteristic, bytes);
        }
        @Override public void onCharacteristicWrite(BluetoothGatt value, BluetoothGattCharacteristic characteristic, int status) {
            handler.post(() -> {
                if (value == gatt && SEND.equals(characteristic.getUuid())) listener.writeCompleted(status == BluetoothGatt.GATT_SUCCESS);
            });
        }
    };
    private void received(BluetoothGatt value, BluetoothGattCharacteristic characteristic, byte[] bytes) {
        if (!RECEIVE.equals(characteristic.getUuid()) || bytes == null) return;
        byte[] owned = bytes.clone();
        long timestamp = System.currentTimeMillis();
        handler.post(() -> { if (value == gatt && subscribed) listener.notification(owned, timestamp); });
    }
}
