package dev.marc.ce.overlay;

import android.app.Activity;
import android.app.Application;
import android.content.Context;
import android.os.Bundle;
import android.os.IBinder;
import android.util.Log;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowManager;

/**
 * Embedded mode: windows attached to the host app's own Activity.
 *
 * <p>{@code TYPE_APPLICATION_PANEL} hanging off a token we already own needs no
 * permission; {@code TYPE_APPLICATION_OVERLAY} - drawing over <em>other</em>
 * apps - is what would.
 *
 * <p>This class also owns the Activity tracking, so that nothing above it is
 * Activity-scoped. Rotation destroys and recreates the Activity; if scan state
 * lived there a rotate would throw away the scan. The windows move from the old
 * Activity to the new one and everything else simply stays.
 */
final class ActivityWindowHost
        implements WindowHost, Application.ActivityLifecycleCallbacks {

    private final Application application;
    private final OverlayController controller;

    private Activity currentActivity;

    ActivityWindowHost(Application application, OverlayController controller) {
        this.application = application;
        this.controller = controller;
    }

    void start() {
        application.registerActivityLifecycleCallbacks(this);
    }

    @Override
    public void release() {
        application.unregisterActivityLifecycleCallbacks(this);
        currentActivity = null;
    }

    // -- WindowHost ----------------------------------------------------------

    @Override
    public Context viewContext() {
        return currentActivity != null ? currentActivity : application;
    }

    @Override
    public WindowManager windowManager() {
        // The Activity's WindowManager, not the Application's: only the
        // Activity-scoped one carries the right display and insets in
        // multi-window and multi-display setups.
        Context context = currentActivity != null ? currentActivity : application;
        return (WindowManager) context.getSystemService(Context.WINDOW_SERVICE);
    }

    @Override
    public boolean ready() {
        return currentActivity != null && !currentActivity.isFinishing() && token() != null;
    }

    private IBinder token() {
        if (currentActivity == null) {
            return null;
        }
        return currentActivity.getWindow().getDecorView().getWindowToken();
    }

    @Override
    public void configureBubble(WindowManager.LayoutParams params) {
        params.type = WindowManager.LayoutParams.TYPE_APPLICATION_PANEL;
        // Not focusable: the bubble must never steal the IME or the back key
        // from the app being inspected. A WRAP_CONTENT window only covers its
        // own bounds, so everything else still reaches the host app.
        params.flags = WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                | WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS;
        params.token = token();
    }

    @Override
    public void configurePanel(WindowManager.LayoutParams params) {
        params.type = WindowManager.LayoutParams.TYPE_APPLICATION_PANEL;
        // Focusable, unlike the bubble: this is the window that has to take taps
        // and raise the soft keyboard for the value fields.
        //
        // FLAG_NOT_TOUCH_MODAL is not optional. A focusable window without it is
        // touch-modal: it swallows every touch outside its own bounds, so the
        // app being inspected stops responding the moment the panel opens -
        // which defeats the entire point, since the workflow is "change a value
        // in the app, then narrow the scan".
        params.flags = WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL
                | WindowManager.LayoutParams.FLAG_LAYOUT_INSET_DECOR;
        params.token = token();
    }

    @Override
    public boolean addBubbleFallback(BubbleView bubble, int x, int y) {
        if (currentActivity == null) {
            return false;
        }
        // Some vendor ROMs refuse a panel window. Falling back to a child of the
        // Activity's content view keeps the tool usable; it just cannot extend
        // past the Activity bounds.
        ViewGroup content = currentActivity.findViewById(android.R.id.content);
        if (content == null) {
            return false;
        }
        bubble.attachToContentView(content, x, y);
        return true;
    }

    // -- Activity lifecycle --------------------------------------------------

    @Override
    public void onActivityResumed(Activity activity) {
        if (activity == currentActivity) {
            return;
        }
        controller.detachWindows();
        currentActivity = activity;
        attachWhenTokenExists(activity);
    }

    private void attachWhenTokenExists(Activity activity) {
        View decor = activity.getWindow().getDecorView();
        if (decor.getWindowToken() == null) {
            // Too early - the decor view has no token until it is attached.
            decor.post(() -> {
                if (activity == currentActivity) {
                    attachWhenTokenExists(activity);
                }
            });
            return;
        }
        try {
            controller.attachWindows();
        } catch (Throwable t) {
            // A debug tool must never be the reason the host app dies.
            Log.w(NativeBridge.TAG, "overlay attach failed", t);
        }
    }

    @Override
    public void onActivityPaused(Activity activity) {
        // Detach on pause rather than stop: the host app launching a dialog
        // Activity would otherwise leak this window.
        if (activity == currentActivity) {
            controller.detachWindows();
            currentActivity = null;
        }
    }

    @Override
    public void onActivityDestroyed(Activity activity) {
        if (activity == currentActivity) {
            controller.detachWindows();
            currentActivity = null;
        }
    }

    @Override
    public void onActivityCreated(Activity activity, Bundle savedInstanceState) {
    }

    @Override
    public void onActivityStarted(Activity activity) {
    }

    @Override
    public void onActivityStopped(Activity activity) {
    }

    @Override
    public void onActivitySaveInstanceState(Activity activity, Bundle outState) {
    }
}
