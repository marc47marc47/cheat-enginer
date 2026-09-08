package dev.marc.ce.app;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;
import android.provider.Settings;
import android.util.Log;
import android.widget.Toast;

import dev.marc.ce.overlay.NativeBridge;
import dev.marc.ce.overlay.OverlayController;

/**
 * Owns the floating engine while the user is somewhere else entirely.
 *
 * <h3>Why a foreground service and not a plain one</h3>
 * The whole point is to be usable while another app is in front. From API 26 a
 * started service belonging to a backgrounded app is stopped by the system
 * within about a minute - and stopping this one takes the overlay windows down
 * with it. A foreground service is what survives, and it comes with the bonus
 * of a Stop control the user can always reach.
 *
 * <p>{@code specialUse} is the only honest type: {@code shortService} caps out
 * at a few minutes and every other type would misdescribe what this does. It
 * requires the {@code <property>} subtype element in the manifest, and on API
 * 34+ the type passed to {@code startForeground} must match the one declared
 * there or the call throws.
 */
public final class EngineService extends Service {

    public static final String ACTION_STOP = "dev.marc.ce.app.STOP";

    private static final String CHANNEL = "ce-engine";
    private static final int NOTIFICATION_ID = 41;

    /**
     * Read by the launcher to render its status block. A static field is enough:
     * there is exactly one process and exactly one of these.
     */
    public static volatile boolean running;

    @Override
    public void onCreate() {
        super.onCreate();
        if (!Settings.canDrawOverlays(this)) {
            // The grant can be revoked while the service is not looking.
            Toast.makeText(this, "Overlay permission not granted", Toast.LENGTH_LONG).show();
            stopSelf();
            return;
        }
        if (!NativeBridge.AVAILABLE) {
            Toast.makeText(this, "Native engine missing for this ABI", Toast.LENGTH_LONG).show();
            stopSelf();
            return;
        }

        startForegroundCompat();

        // The bubble appears collapsed, exactly as in embedded mode; the panel
        // opens on a tap. Nothing attaches to the target process until then,
        // so a service left running costs no scan thread.
        OverlayController.installGlobal(getApplication());
        running = true;
        Log.i(NativeBridge.TAG, "engine service up, pid=" + android.os.Process.myPid());
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent != null && ACTION_STOP.equals(intent.getAction())) {
            stopSelf();
        }
        // Not sticky: a restart with a null intent would put the overlay back on
        // screen after the user explicitly stopped it.
        return START_NOT_STICKY;
    }

    @Override
    public void onDestroy() {
        running = false;
        OverlayController controller = OverlayController.get();
        if (controller != null) {
            // Windows come off now - WindowManager is main-thread only - and the
            // engine's threads are joined on a worker. Session::shutdown joins
            // the scan thread, and a scan across a whole shared process can take
            // seconds; doing that here would be an ANR.
            controller.shutdownAsync(null);
        }
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    // -- notification --------------------------------------------------------

    private void startForegroundCompat() {
        Notification notification = buildNotification();
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIFICATION_ID, notification,
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE);
        } else {
            startForeground(NOTIFICATION_ID, notification);
        }
    }

    private Notification buildNotification() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            NotificationChannel channel = new NotificationChannel(
                    CHANNEL, "Cheat Engine", NotificationManager.IMPORTANCE_LOW);
            channel.setShowBadge(false);
            getSystemService(NotificationManager.class).createNotificationChannel(channel);
        }

        Intent stop = new Intent(this, EngineService.class).setAction(ACTION_STOP);
        PendingIntent stopIntent = PendingIntent.getService(
                this, 0, stop, PendingIntent.FLAG_IMMUTABLE);

        Intent open = new Intent(this, LauncherActivity.class)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TOP);
        PendingIntent openIntent = PendingIntent.getActivity(
                this, 1, open, PendingIntent.FLAG_IMMUTABLE);

        Notification.Builder builder = Build.VERSION.SDK_INT >= Build.VERSION_CODES.O
                ? new Notification.Builder(this, CHANNEL)
                : new Notification.Builder(this);
        return builder
                .setContentTitle("Cheat Engine attached")
                .setContentText("pid " + android.os.Process.myPid() + " - tap the bubble to scan")
                .setSmallIcon(android.R.drawable.ic_menu_search)
                .setOngoing(true)
                .setContentIntent(openIntent)
                .addAction(new Notification.Action.Builder(null, "Stop", stopIntent).build())
                .build();
    }

    static void start(Context context) {
        Intent intent = new Intent(context, EngineService.class);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            context.startForegroundService(intent);
        } else {
            context.startService(intent);
        }
    }

    static void stop(Context context) {
        context.stopService(new Intent(context, EngineService.class));
    }
}
