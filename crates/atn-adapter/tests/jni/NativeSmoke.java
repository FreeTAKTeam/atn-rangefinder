import java.nio.file.Files;
import java.nio.file.Path;
import org.freetak.atn.NativeCore;

/** Exercises the real exported JNI ABI with the original synthetic configuration. */
public final class NativeSmoke {
    /** ATN protocol GATT identity, as the Android transport reports it. */
    private static final String GATT = "\"serviceUuid\":\"d973f2e0-b19e-11e2-9e96-0800200c9a66\","
        + "\"notifyCharacteristic\":\"d973f2e1-b19e-11e2-9e96-0800200c9a66\","
        + "\"writeCharacteristic\":\"d973f2e2-b19e-11e2-9e96-0800200c9a66\"";
    private static void require(String response, String expected) {
        if (!response.contains(expected)) throw new AssertionError("JNI response failed expectation: " + expected);
    }
    public static void main(String[] args) throws Exception {
        if (!NativeCore.available()) throw new AssertionError("Native library missing");
        String config = Files.readString(Path.of(args[0]));
        require(NativeCore.dispatch("{\"cmd\":\"configure\",\"nowMs\":10000,\"config\":" + config + "}"), "\"configured\":true");
        require(NativeCore.dispatch("{\"cmd\":\"connected\",\"connectionId\":\"synthetic-jni\",\"writeCapacity\":509,\"nowMs\":10000," + GATT + "}"), "\"readOnly\":true");
        String observed = NativeCore.dispatch("{\"cmd\":\"notification\",\"characteristic\":\"d973f2e1-b19e-11e2-9e96-0800200c9a66\",\"bytes\":[1,1,0,120,255,251,0,210],\"observedAtMs\":10000,\"nowMs\":10000}");
        require(observed, "\"pitch\":-5");
        require(observed, "\"sourceSession\":\"synthetic-jni\"");
        require(NativeCore.dispatch("{\"cmd\":\"setStyle\",\"markerType\":1,\"color\":3,\"nowMs\":10000}"), "\"ok\":false");
        require(NativeCore.dispatch("{\"cmd\":\"hostEvent\",\"nowMs\":10000,\"envelope\":{\"protocolVersion\":1,\"event\":\"map.session.started\",\"payload\":{}}}"), "\"ignored\"");
        require(NativeCore.dispatch("malformed ☃"), "\"ok\":false");
        require(NativeCore.dispatch("{\"cmd\":\"status\",\"nowMs\":10000}"), "\"ok\":true");
        System.out.println("JNI ABI/configuration/read-only observation/error smoke: PASS");
    }
}
