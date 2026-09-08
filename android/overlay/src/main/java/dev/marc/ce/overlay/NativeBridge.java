package dev.marc.ce.overlay;

import android.util.Log;

/**
 * Every call into the Rust engine.
 *
 * <p>The library is loaded once, lazily, and a failure is not fatal: a debug
 * APK built for an ABI we did not ship must still start, just without the
 * overlay. {@link #AVAILABLE} says which happened.
 *
 * <p>Records returned by {@link #resultsPage} and {@link #tablePage} are packed
 * little-endian, {@link #RECORD_SIZE} bytes each - see {@link ScanRecord}. The
 * alternative, one JNI call per row, costs more than the whole scan on a list
 * of any size.
 */
public final class NativeBridge {

    public static final String TAG = "ce-overlay";

    /** Bytes per packed record. Must match RECORD_SIZE in src/android/mod.rs. */
    public static final int RECORD_SIZE = 24;

    public static final boolean AVAILABLE;

    static {
        boolean loaded;
        try {
            System.loadLibrary("ce_engine");
            loaded = true;
        } catch (UnsatisfiedLinkError e) {
            Log.w(TAG, "native engine not present; overlay disabled", e);
            loaded = false;
        }
        AVAILABLE = loaded;
    }

    private NativeBridge() {
    }

    // -- lifecycle ----------------------------------------------------------

    /** Attach to this process. Returns 0 on failure. */
    public static native long nativeInit();

    /** Stops the engine's threads, then frees it. Never call with a live handle in use. */
    public static native void nativeDestroy(long handle);

    public static native String nativeVersionLine();

    public static native int nativeSelfPid();

    /** Takes and clears the last error, or null. */
    public static native String nativeLastError(long handle);

    // -- scanning -----------------------------------------------------------

    public static native boolean nativeStartScan(
            long handle, int valueType, int scanType, String target, boolean restart);

    /** {@code [state, scannedRegions, totalRegions, found, truncated]}. */
    public static native long[] nativeScanStatus(long handle);

    public static native void nativeCancelScan(long handle);

    public static native void nativeResetScan(long handle);

    /** Result count, or -1 while a scan owns the scanner. */
    public static native int nativeResultCount(long handle);

    public static native byte[] nativeResultsPage(long handle, int offset, int count);

    // -- memory -------------------------------------------------------------

    public static native byte[] nativeReadBytes(long handle, long address, int len);

    public static native boolean nativeWriteValue(
            long handle, long address, int valueType, String text);

    // -- address table ------------------------------------------------------

    public static native void nativeTableAdd(
            long handle, long address, int valueType, String description);

    public static native void nativeTableRemove(long handle, int index);

    public static native boolean nativeTableToggleFreeze(long handle, int index);

    public static native boolean nativeTableSetValue(long handle, int index, String text);

    public static native byte[] nativeTablePage(long handle);

    public static native String[] nativeTableDescriptions(long handle);

    public static native boolean nativeTableSave(long handle, String path);

    public static native boolean nativeTableLoad(long handle, String path);

    // -- labels, so the spinners cannot drift from the engine ---------------

    public static native String[] nativeValueTypeLabels();

    public static native String[] nativeScanTypeLabels();

    /** Bit set of scan-mode indices that require a typed value. */
    public static native int nativeScanTypesNeedingValue();
}
