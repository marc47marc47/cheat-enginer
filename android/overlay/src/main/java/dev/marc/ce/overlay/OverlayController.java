package dev.marc.ce.overlay;

import android.app.Application;
import android.content.Context;
import android.content.SharedPreferences;
import android.graphics.PixelFormat;
import android.os.Handler;
import android.os.HandlerThread;
import android.util.Log;
import android.view.Gravity;
import android.view.WindowManager;

import java.io.File;

/**
 * Process-wide owner of the overlay: the engine session, the bubble and the
 * panel, and their position.
 *
 * <p>Where the windows go is not decided here - see {@link WindowHost}. This
 * class knows only that it has one collapsed window and one expanded window and
 * asks the host to type and flag them.
 */
public final class OverlayController {

    private static final String PREFS = "ce_overlay_prefs";
    private static final String KEY_X = "bubble_x";
    private static final String KEY_Y = "bubble_y";

    private static OverlayController instance;

    private final Application application;
    private final SharedPreferences prefs;

    /** Opaque Rust session pointer. Created lazily, on first expand. */
    private volatile long sessionHandle;

    private WindowHost host;
    private BubbleView bubble;
    private OverlayPanel panel;
    private WindowManager.LayoutParams bubbleParams;
    private boolean expanded;

    private OverlayController(Application application) {
        this.application = application;
        this.prefs = application.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    // -- installation --------------------------------------------------------

    /**
     * Embedded mode. Called from {@link OverlayInstallProvider} before the host
     * app's {@code Application.onCreate}, so the host's source never mentions
     * this library.
     */
    static synchronized void install(Application application) {
        if (instance != null) {
            return;
        }
        instance = new OverlayController(application);
        ActivityWindowHost host = new ActivityWindowHost(application, instance);
        instance.host = host;
        host.start();
    }

    /**
     * Standalone mode. Called from the engine APK's own Service once the
     * {@code SYSTEM_ALERT_WINDOW} grant is confirmed.
     *
     * <p>Idempotent, and it replaces an Activity host if one happens to be
     * installed - which matters because the engine APK ships the same AAR and
     * would otherwise end up with two bubbles.
     */
    public static synchronized OverlayController installGlobal(Application application) {
        if (instance == null) {
            instance = new OverlayController(application);
        }
        if (!(instance.host instanceof SystemOverlayWindowHost)) {
            instance.replaceHost(new SystemOverlayWindowHost(application));
        }
        return instance;
    }

    public static OverlayController get() {
        return instance;
    }

    private synchronized void replaceHost(WindowHost next) {
        detachWindows();
        if (host != null) {
            host.release();
        }
        host = next;
        // A new host means views built from the old one's Context are stale.
        bubble = null;
        panel = null;
        attachWindows();
    }

    // -- engine session ------------------------------------------------------

    /**
     * The engine handle, created on first use.
     *
     * <p>Deferred rather than created at startup so a debug APK that never
     * opens the overlay pays nothing: no attach, and no freeze thread ticking
     * every 100 ms for the life of the process.
     */
    public long session() {
        long handle = sessionHandle;
        if (handle != 0) {
            return handle;
        }
        synchronized (this) {
            if (sessionHandle == 0) {
                sessionHandle = NativeBridge.nativeInit();
                if (sessionHandle == 0) {
                    Log.e(NativeBridge.TAG, "could not attach to this process");
                } else {
                    Log.i(NativeBridge.TAG, "attached: " + NativeBridge.nativeVersionLine()
                            + " pid=" + NativeBridge.nativeSelfPid());
                    // Bring back whatever the user had saved last run. Scan
                    // results are intentionally not persisted; the address
                    // table is the expensive part to rebuild.
                    NativeBridge.nativeTableLoad(sessionHandle, tablePath());
                }
            }
            return sessionHandle;
        }
    }

    public String tablePath() {
        return new File(application.getFilesDir(), "ce_addresses.json").getAbsolutePath();
    }

    /**
     * Stops the engine's threads and releases it. Safe to call twice.
     *
     * <p><b>This blocks.</b> {@code Session::shutdown} joins the scan thread and
     * the 100 ms freeze thread, and a scan in flight over a whole process's
     * address space can take seconds. Callers on the main thread must use
     * {@link #shutdownAsync}.
     */
    public synchronized void shutdown() {
        collapse();
        detachWindows();
        if (host != null) {
            host.release();
        }
        if (sessionHandle != 0) {
            long handle = sessionHandle;
            sessionHandle = 0;
            NativeBridge.nativeDestroy(handle);
        }
    }

    /**
     * Takes the windows down immediately and joins the engine's threads off the
     * main thread.
     *
     * <p>The window removal has to be synchronous - WindowManager is
     * main-thread-only - but the native join must not be, or stopping the
     * service while a scan is running is an ANR.
     */
    public void shutdownAsync(Runnable onDone) {
        long handle = sessionHandle;
        if (handle != 0) {
            // Cancel first so the scan thread is already unwinding by the time
            // the join happens.
            NativeBridge.nativeCancelScan(handle);
        }
        collapse();
        detachWindows();
        HandlerThread thread = new HandlerThread("ce-shutdown");
        thread.start();
        new Handler(thread.getLooper()).post(() -> {
            shutdown();
            if (onDone != null) {
                onDone.run();
            }
            thread.quitSafely();
        });
    }

    // -- windows -------------------------------------------------------------

    /** Adds the bubble, and the panel too if it was open. Idempotent. */
    synchronized void attachWindows() {
        if (host == null || !host.ready()) {
            return;
        }
        if (bubble == null) {
            bubble = new BubbleView(host.viewContext(), this);
        } else if (bubble.getParent() != null) {
            return;
        }

        bubbleParams = new WindowManager.LayoutParams(
                WindowManager.LayoutParams.WRAP_CONTENT,
                WindowManager.LayoutParams.WRAP_CONTENT,
                0,
                0,
                PixelFormat.TRANSLUCENT);
        host.configureBubble(bubbleParams);
        bubbleParams.gravity = Gravity.TOP | Gravity.START;
        bubbleParams.x = prefs.getInt(KEY_X, dp(16));
        bubbleParams.y = prefs.getInt(KEY_Y, dp(120));

        try {
            host.windowManager().addView(bubble, bubbleParams);
        } catch (WindowManager.BadTokenException | IllegalStateException
                 | SecurityException e) {
            Log.w(NativeBridge.TAG, "bubble window refused", e);
            if (!host.addBubbleFallback(bubble, bubbleParams.x, bubbleParams.y)) {
                bubble = null;
                return;
            }
        }

        if (expanded) {
            showPanel();
        }
    }

    synchronized void detachWindows() {
        hidePanel();
        if (bubble != null && host != null) {
            bubble.detach(host.windowManager());
        }
    }

    void toggle() {
        if (expanded) {
            collapse();
        } else {
            expand();
        }
    }

    void expand() {
        expanded = true;
        showPanel();
    }

    void collapse() {
        expanded = false;
        hidePanel();
    }

    private void showPanel() {
        if (host == null || !host.ready() || panel != null) {
            return;
        }
        if (session() == 0) {
            return;
        }
        panel = new OverlayPanel(host.viewContext(), this);

        WindowManager.LayoutParams params = new WindowManager.LayoutParams(
                WindowManager.LayoutParams.MATCH_PARENT,
                WindowManager.LayoutParams.WRAP_CONTENT,
                0,
                0,
                PixelFormat.TRANSLUCENT);
        host.configurePanel(params);
        params.gravity = Gravity.BOTTOM | Gravity.START;
        params.softInputMode = WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE
                | WindowManager.LayoutParams.SOFT_INPUT_STATE_UNSPECIFIED;

        try {
            host.windowManager().addView(panel, params);
        } catch (WindowManager.BadTokenException | IllegalStateException
                 | SecurityException e) {
            Log.w(NativeBridge.TAG, "panel window refused", e);
            panel = null;
            return;
        }
        panel.onShown();
    }

    private void hidePanel() {
        if (panel == null) {
            return;
        }
        panel.onHidden();
        try {
            if (host != null) {
                host.windowManager().removeViewImmediate(panel);
            }
        } catch (IllegalArgumentException ignored) {
            // Already gone with its Activity.
        }
        panel = null;
    }

    void onBubbleMoved(int x, int y) {
        if (bubbleParams == null || host == null) {
            return;
        }
        bubbleParams.x = x;
        bubbleParams.y = y;
        prefs.edit().putInt(KEY_X, x).putInt(KEY_Y, y).apply();
        try {
            host.windowManager().updateViewLayout(bubble, bubbleParams);
        } catch (IllegalArgumentException ignored) {
            // Detached mid-drag.
        }
    }

    int dp(int value) {
        Context context = host != null ? host.viewContext() : application;
        return Math.round(value * context.getResources().getDisplayMetrics().density);
    }
}
