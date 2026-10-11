package org.freetak.atn;

/** The protocol/session owner is Rust. Android never reimplements the wire codec. */
public final class NativeCore {
    public static final int MAX_REQUEST_BYTES = 393_216;
    private static final boolean AVAILABLE;
    static {
        boolean loaded;
        try { System.loadLibrary("atn_adapter"); loaded = true; }
        catch (UnsatisfiedLinkError error) { loaded = false; }
        AVAILABLE = loaded;
    }
    private NativeCore() {}
    public static boolean available() { return AVAILABLE; }
    public static native String dispatch(String requestJson);
}
