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

    // -- Speed-tab vsync experiments (see android/TODO-vsync.md) --------------
    /** Frame-pacing pulse: a bg thread nudging the main Looper at a fine rate. */
    private volatile boolean pacing;
    private Thread pacer;
    /** Requested display refresh (Hz); 0 = system default. Best-effort. */
    private float desiredRefreshRate;

    // -- Game speed + pause (held here so it survives panel rebuilds and so the
    //    panel can pause on open regardless of which tab is showing) -----------
    /** Desired running speed (applied when not paused). 1.0 = real time. */
    private double speedFactor = 1.0;
    /** True while the game is frozen (panel open + pauseWhileOpen). */
    private boolean gamePaused;
    /** Freeze the game while the panel is open (user's request). Default on. */
    private boolean pauseWhileOpen = true;
    private boolean speedInstalled;
    private boolean speedUnsupported;

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
        setFramePacing(false); // never let the pacer thread outlive the overlay
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
        // Reapply a standing refresh-rate request across panel reopen (0 = default).
        params.preferredRefreshRate = desiredRefreshRate;

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

    // -- Speed-tab vsync experiments -----------------------------------------

    /**
     * Frame-pacing pulse. The game's per-frame step is a {@code postDelayed}
     * message whose due time is already scaled by the clock hook, but the main
     * thread sleeps in {@code epoll_pwait(-1)} between vsyncs (60 Hz) so it never
     * runs the overdue step early — movement stays 60 fps. A background thread
     * that pokes the main Looper awake ~1000×/s lets that scaled step dispatch at
     * its scaled rate, so movement tracks the factor (4× faster / 0.25× slower).
     * Costs CPU while on; independent of the factor (no visible effect at 1×).
     */
    synchronized void setFramePacing(boolean on) {
        if (on == pacing) {
            return;
        }
        pacing = on;
        if (on) {
            final Handler main = new Handler(application.getMainLooper());
            final Runnable noop = () -> {};
            pacer = new Thread(() -> {
                // Nudge the main Looper awake every ~PULSE_NS. Each post wakes its
                // epoll_pwait so MessageQueue re-reads the (scaled) uptime and
                // dispatches the game's overdue, clock-scaled frame step — instead
                // of it sleeping until the next vsync (which caps movement at 60fps).
                // The BACKGROUND thread does the busy-wait, so the main thread stays
                // free to actually run the extra steps + draws (pegging the main
                // thread instead starves them). Fine granularity matters: a plain
                // Thread.sleep(1) is too coarse and wakes no faster than vsync.
                final long PULSE_NS = 2_000_000L; // 2 ms → ~500 wakes/s
                long next = System.nanoTime();
                while (pacing) {
                    main.post(noop);
                    next += PULSE_NS;
                    while (pacing && System.nanoTime() < next) {
                        // busy-wait for fine timing (this thread only)
                    }
                }
            }, "ce-frame-pacer");
            pacer.setDaemon(true);
            pacer.start();
        } else {
            Thread t = pacer;
            pacer = null;
            if (t != null) {
                t.interrupt();
            }
        }
    }

    boolean isFramePacing() {
        return pacing;
    }

    /**
     * Best-effort display-refresh request (Hz; 0 = system default). Sets the
     * overlay window's {@code preferredRefreshRate}; the system may lower the
     * whole display to it, slowing the game's vsync — but only if the panel is
     * showing a supported mode. On a single-mode display (typical emulator) this
     * is a no-op.
     */
    synchronized void setPreferredRefreshRate(float hz) {
        desiredRefreshRate = hz;
        applyRefreshRate();
    }

    float preferredRefreshRate() {
        return desiredRefreshRate;
    }

    private void applyRefreshRate() {
        if (panel == null || host == null) {
            return;
        }
        android.view.ViewGroup.LayoutParams lp = panel.getLayoutParams();
        if (!(lp instanceof WindowManager.LayoutParams)) {
            return;
        }
        WindowManager.LayoutParams wlp = (WindowManager.LayoutParams) lp;
        wlp.preferredRefreshRate = desiredRefreshRate;
        try {
            host.windowManager().updateViewLayout(panel, wlp);
        } catch (IllegalArgumentException ignored) {
            // Panel detached.
        }
    }

    // -- Game speed + pause --------------------------------------------------

    private void ensureSpeedInstalled() {
        if (speedInstalled || speedUnsupported) {
            return;
        }
        speedInstalled = NativeBridge.nativeSpeedInstall();
        speedUnsupported = !speedInstalled; // e.g. armeabi-v7a (Elf32) can't hook
    }

    /**
     * The slowest speed used for "pause". A true 0 freezes the process's
     * monotonic clock, which also freezes the overlay's own rendering (shared
     * main thread) — verified on device — so pause slows to a crawl instead.
     */
    private static final double PAUSE_FACTOR = 0.15;

    /** PAUSE_FACTOR (a crawl) while paused, otherwise the desired running speed. */
    private void applyEffectiveSpeed() {
        if (speedUnsupported) {
            return;
        }
        NativeBridge.nativeSpeedSet(gamePaused ? PAUSE_FACTOR : speedFactor);
    }

    boolean speedUnsupported() {
        return speedUnsupported;
    }

    double speedFactor() {
        return speedFactor;
    }

    boolean gamePaused() {
        return gamePaused;
    }

    boolean pauseWhileOpen() {
        return pauseWhileOpen;
    }

    /** Desired running speed (chips / +/- buttons). Clamped to [0.1, 8] on a 0.05 grid. */
    void setSpeedFactor(double f) {
        if (f < 0.1) {
            f = 0.1;
        } else if (f > 8.0) {
            f = 8.0;
        }
        speedFactor = Math.round(f * 20.0) / 20.0; // snap to 0.05
        ensureSpeedInstalled();
        applyEffectiveSpeed();
    }

    /** Toggle "freeze the game while the panel is open". Reflects immediately if it is. */
    void setPauseWhileOpen(boolean on) {
        pauseWhileOpen = on;
        if (panel != null) { // panel currently showing
            if (on) {
                ensureSpeedInstalled();
                gamePaused = !speedUnsupported;
            } else {
                gamePaused = false;
            }
            applyEffectiveSpeed();
        }
    }

    /** Panel opened → freeze the game if the toggle is on. */
    void onSpeedPanelShown() {
        if (pauseWhileOpen) {
            ensureSpeedInstalled();
            gamePaused = !speedUnsupported;
            applyEffectiveSpeed();
        }
    }

    /** Panel closed → resume the game. */
    void onSpeedPanelHidden() {
        if (gamePaused) {
            gamePaused = false;
            applyEffectiveSpeed();
        }
    }

    int dp(int value) {
        Context context = host != null ? host.viewContext() : application;
        return Math.round(value * context.getResources().getDisplayMetrics().density);
    }
}
