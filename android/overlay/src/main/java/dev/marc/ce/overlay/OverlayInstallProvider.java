package dev.marc.ce.overlay;

import android.app.Application;
import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.net.Uri;
import android.util.Log;

/**
 * Installs the overlay with no help from the host app.
 *
 * <p>A ContentProvider's {@code onCreate} runs before {@code
 * Application.onCreate}, which is what makes zero integration possible: the
 * host app's source never mentions this library, so adding it as
 * {@code debugImplementation} is enough to keep it out of release builds
 * entirely - there is no call site that would fail to compile.
 *
 * <p>Chosen over androidx.startup because that would be a runtime dependency,
 * and the whole point of this library is to add none.
 */
public final class OverlayInstallProvider extends ContentProvider {

    @Override
    public boolean onCreate() {
        Context context = getContext();
        if (context == null) {
            return true;
        }
        if (!NativeBridge.AVAILABLE) {
            // No .so for this ABI. The host app must still start normally.
            Log.w(NativeBridge.TAG, "overlay not installed: native engine unavailable");
            return true;
        }
        Context app = context.getApplicationContext();
        if (app instanceof Application) {
            try {
                OverlayController.install((Application) app);
            } catch (Throwable t) {
                // A debug tool must never be the reason an app fails to launch.
                Log.e(NativeBridge.TAG, "overlay install failed", t);
            }
        }
        return true;
    }

    @Override
    public Cursor query(Uri uri, String[] projection, String selection,
                        String[] selectionArgs, String sortOrder) {
        return null;
    }

    @Override
    public String getType(Uri uri) {
        return null;
    }

    @Override
    public Uri insert(Uri uri, ContentValues values) {
        return null;
    }

    @Override
    public int delete(Uri uri, String selection, String[] selectionArgs) {
        return 0;
    }

    @Override
    public int update(Uri uri, ContentValues values, String selection, String[] selectionArgs) {
        return 0;
    }
}
