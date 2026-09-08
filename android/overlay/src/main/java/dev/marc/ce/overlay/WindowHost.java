package dev.marc.ce.overlay;

import android.content.Context;
import android.view.WindowManager;

/**
 * Where the overlay's two windows get put.
 *
 * <p>This is the one thing that differs between the library's two deployment
 * modes, and it differs completely:
 *
 * <ul>
 *   <li><b>Embedded</b> ({@link ActivityWindowHost}) - the AAR is compiled into
 *       the app being inspected. The windows hang off that app's own Activity
 *       token as {@code TYPE_APPLICATION_PANEL}, which needs no permission at
 *       all.</li>
 *   <li><b>Standalone</b> ({@link SystemOverlayWindowHost}) - the engine ships
 *       as its own APK sharing a process with the target via
 *       {@code sharedUserId}. There is no Activity to hang off: a shared
 *       process has one {@code Application} object <em>per package</em>, so the
 *       engine package's {@code ActivityLifecycleCallbacks} never sees the
 *       target's Activities and can never obtain its window token. The windows
 *       must be {@code TYPE_APPLICATION_OVERLAY}, which costs a
 *       {@code SYSTEM_ALERT_WINDOW} grant.</li>
 * </ul>
 *
 * <p>Note that "where does the window live" and "whose memory can be read" are
 * independent questions. The overlay permission has nothing to do with the
 * shared process and the shared process nothing to do with the permission;
 * either can fail on its own.
 */
interface WindowHost {

    /** Context to build views with. Must carry a theme. */
    Context viewContext();

    /** The WindowManager the two windows are added to. */
    WindowManager windowManager();

    /** Whether a window can be added right now. */
    boolean ready();

    /** Sets type, token and flags for the collapsed bubble. */
    void configureBubble(WindowManager.LayoutParams params);

    /** Sets type, token and flags for the expanded panel. */
    void configurePanel(WindowManager.LayoutParams params);

    /**
     * Last resort when {@link WindowManager#addView} is refused. Returns true
     * if the host placed the view some other way.
     */
    boolean addBubbleFallback(BubbleView bubble, int x, int y);

    /** Releases whatever the host registered. */
    void release();
}
