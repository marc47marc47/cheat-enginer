package dev.marc.ce.overlay;

import android.view.View;
import android.view.ViewGroup;
import android.view.ViewParent;
import android.view.WindowManager;

/**
 * Detaching a view that may have been added either to a {@link WindowManager}
 * or, on the fallback path, to the Activity's content view.
 *
 * <p>The two cases cannot be told apart by casting: a view added through a
 * {@code WindowManager} has a {@code ViewRootImpl} as its parent, which is a
 * {@link ViewParent} but <em>not</em> a {@link ViewGroup}. Casting blindly
 * throws {@code ClassCastException} inside {@code onActivityPaused} and takes
 * the host app down with it.
 *
 * <p>Both removals also throw when the view is already gone, which happens
 * routinely: an Activity being destroyed takes its windows with it before the
 * lifecycle callback arrives.
 */
final class ViewParentSafeRemove {

    private ViewParentSafeRemove() {
    }

    static void remove(View view, WindowManager windowManager) {
        ViewParent parent = view.getParent();
        if (parent == null) {
            return;
        }
        if (parent instanceof ViewGroup) {
            ((ViewGroup) parent).removeView(view);
            return;
        }
        try {
            windowManager.removeViewImmediate(view);
        } catch (IllegalArgumentException ignored) {
            // Already torn down with its Activity.
        }
    }
}
