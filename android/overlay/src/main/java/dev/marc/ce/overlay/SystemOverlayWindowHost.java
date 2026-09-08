package dev.marc.ce.overlay;

import android.app.Application;
import android.content.Context;
import android.hardware.display.DisplayManager;
import android.os.Build;
import android.view.ContextThemeWrapper;
import android.view.Display;
import android.view.WindowManager;

/**
 * Standalone mode: windows that float over whatever is on screen.
 *
 * <p>Used when the engine ships as its own APK and reaches the target process
 * through {@code sharedUserId} plus a matching {@code android:process}. There
 * is no Activity token to borrow - the engine package's
 * {@code ActivityLifecycleCallbacks} never fire for the other package's
 * Activities, because a shared process has one {@code Application} object and
 * one ClassLoader <em>per package</em>. So the windows are
 * {@code TYPE_APPLICATION_OVERLAY}, added through the application's own
 * WindowManager, and the price is a {@code SYSTEM_ALERT_WINDOW} grant.
 *
 * <p><b>Never make one of these windows full-screen.</b> Since Android 12 the
 * system blocks touches that pass through an overlay to the app below. Windows
 * owned by the same UID are exempt, which is true here, but a content-sized
 * bubble and a bottom-anchored panel keep the tool out of that argument
 * entirely.
 */
final class SystemOverlayWindowHost implements WindowHost {

    private final Context viewContext;
    private final WindowManager windowManager;

    SystemOverlayWindowHost(Application application) {
        Context base = application;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            // A window context carries the metrics, insets and configuration of
            // the display the window will actually appear on; a bare application
            // context carries none of that.
            //
            // The display has to be named explicitly. The two-argument
            // createWindowContext(type, options) derives it from the receiver
            // and throws UnsupportedOperationException on a Context that is not
            // associated with one - which an Application never is.
            Display display = application.getSystemService(DisplayManager.class)
                    .getDisplay(Display.DEFAULT_DISPLAY);
            base = application.createWindowContext(display,
                    WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY, null);
        }
        // The application context has no theme, so widgets built from it come
        // out unstyled. Wrap explicitly rather than relying on whatever
        // android:theme the host manifest happens to declare.
        this.viewContext = new ContextThemeWrapper(base, android.R.style.Theme_Material);
        this.windowManager = (WindowManager) base.getSystemService(Context.WINDOW_SERVICE);
    }

    @Override
    public Context viewContext() {
        return viewContext;
    }

    @Override
    public WindowManager windowManager() {
        return windowManager;
    }

    @Override
    public boolean ready() {
        // Nothing to wait for: an overlay window needs no Activity and no token.
        return true;
    }

    private static int overlayType() {
        return Build.VERSION.SDK_INT >= Build.VERSION_CODES.O
                ? WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY
                : WindowManager.LayoutParams.TYPE_PHONE;
    }

    @Override
    public void configureBubble(WindowManager.LayoutParams params) {
        params.type = overlayType();
        params.flags = WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                | WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS;
        params.token = null;
    }

    @Override
    public void configurePanel(WindowManager.LayoutParams params) {
        params.type = overlayType();
        // Same reasoning as the embedded host: focusable so the value fields can
        // raise the IME, and FLAG_NOT_TOUCH_MODAL so the game underneath still
        // receives every touch outside the panel.
        //
        // FLAG_ALT_FOCUSABLE_IM is deliberately *not* set - it would make the
        // window focusable for touch but invisible to the input method, and the
        // EditTexts would silently refuse the keyboard.
        params.flags = WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL;
        params.token = null;
    }

    @Override
    public boolean addBubbleFallback(BubbleView bubble, int x, int y) {
        // There is no content view to fall back to. If the WindowManager refused
        // an overlay window the permission is missing or revoked, and the
        // caller reports that.
        return false;
    }

    @Override
    public void release() {
    }
}
