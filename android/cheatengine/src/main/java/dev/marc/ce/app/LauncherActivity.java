package dev.marc.ce.app;

import android.Manifest;
import android.app.Activity;
import android.content.Intent;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.graphics.Color;
import android.graphics.Typeface;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Process;
import android.provider.Settings;
import android.view.Gravity;
import android.view.View;
import android.view.WindowInsets;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;

import dev.marc.ce.overlay.NativeBridge;

/**
 * The Cheat Engine app's own screen: grant the permission, start the engine,
 * jump to the game.
 *
 * <p>It also diagnoses the one thing that silently breaks everything. This APK
 * can only read the game's memory because both packages declare the same
 * {@code sharedUserId} and the same {@code android:process}, which puts them in
 * one OS process - no root, no ptrace, nothing to be denied by SELinux. If that
 * failed (usually because one package was installed before the manifest carried
 * the attribute, so its UID was already fixed) everything still launches and
 * scans simply find nothing. So the status block prints the pid and uid and
 * says outright when the game is not in the same shared user.
 */
public final class LauncherActivity extends Activity {

    private static final String GAME = "dev.marc.ce.game";

    private static final int BG = Color.rgb(16, 18, 22);
    private static final int FG = Color.rgb(222, 228, 236);
    private static final int DIM = Color.rgb(140, 150, 165);
    private static final int WARN = Color.rgb(235, 130, 120);

    private TextView status;
    private Button grantButton;
    private Button startButton;
    private Button stopButton;
    private Button gameButton;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        ScrollView scroll = new ScrollView(this);
        scroll.setBackgroundColor(BG);
        scroll.setFillViewport(true);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            scroll.setOnApplyWindowInsetsListener((v, insets) -> {
                android.graphics.Insets bars = insets.getInsets(WindowInsets.Type.systemBars());
                v.setPadding(dp(20), bars.top + dp(20), dp(20), bars.bottom + dp(20));
                return insets;
            });
        } else {
            scroll.setPadding(dp(20), dp(40), dp(20), dp(20));
        }

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        // Keep the first Button from grabbing focus and auto-scrolling the
        // heading off the top.
        root.setFocusableInTouchMode(true);
        scroll.addView(root, new ScrollView.LayoutParams(
                ScrollView.LayoutParams.MATCH_PARENT,
                ScrollView.LayoutParams.WRAP_CONTENT));

        TextView title = new TextView(this);
        title.setText("Cheat Engine");
        title.setTextColor(FG);
        title.setTextSize(26f);
        title.setTypeface(null, Typeface.BOLD);
        root.addView(title);

        TextView blurb = new TextView(this);
        blurb.setText("Scans the process it lives in. This APK and Dungeon Tap "
                + "declare the same sharedUserId and the same android:process, "
                + "so Android puts them in one process - which is why no root "
                + "is needed and why nothing else on the device is visible.");
        blurb.setTextColor(DIM);
        blurb.setTextSize(13f);
        blurb.setPadding(0, dp(8), 0, dp(16));
        root.addView(blurb);

        status = new TextView(this);
        status.setTypeface(Typeface.MONOSPACE);
        status.setTextSize(12f);
        status.setLineSpacing(dp(2), 1f);
        status.setPadding(dp(12), dp(12), dp(12), dp(12));
        status.setBackgroundColor(Color.rgb(24, 27, 33));
        root.addView(status);

        grantButton = addButton(root, "Grant overlay permission", v -> requestOverlay());
        startButton = addButton(root, "Start engine", v -> startEngine());
        stopButton = addButton(root, "Stop engine", v -> {
            EngineService.stop(this);
            // onDestroy clears the flag; give it a moment before re-reading.
            status.postDelayed(this::renderStatus, 300);
        });
        gameButton = addButton(root, "Open Dungeon Tap", v -> openGame());

        TextView hint = new TextView(this);
        hint.setText("Tap the CE bubble to open the panel. It stays on screen "
                + "over the game. Force-stopping either app kills the shared "
                + "process and the scan with it.");
        hint.setTextColor(DIM);
        hint.setTextSize(12f);
        hint.setPadding(0, dp(16), 0, 0);
        root.addView(hint);

        setContentView(scroll);
    }

    @Override
    protected void onResume() {
        super.onResume();
        // canDrawOverlays is known to lag right after the Settings toggle on
        // some ROMs, so this is re-read every time the screen comes back rather
        // than cached.
        renderStatus();
    }

    // -- status --------------------------------------------------------------

    private void renderStatus() {
        boolean canOverlay = Settings.canDrawOverlays(this);
        StringBuilder text = new StringBuilder();
        text.append("package  ").append(getPackageName()).append('\n');
        text.append("pid      ").append(Process.myPid()).append('\n');
        text.append("uid      ").append(Process.myUid()).append('\n');
        text.append("engine   ")
                .append(NativeBridge.AVAILABLE ? NativeBridge.nativeVersionLine() : "UNAVAILABLE")
                .append('\n');
        text.append("overlay  ").append(canOverlay ? "granted" : "NOT GRANTED").append('\n');
        text.append("service  ").append(EngineService.running ? "running" : "stopped").append('\n');

        boolean gameShared = false;
        boolean gameInstalled = false;
        try {
            ApplicationInfo info = getPackageManager().getApplicationInfo(GAME, 0);
            gameInstalled = true;
            gameShared = info.uid == Process.myUid();
            text.append("game     ").append(gameShared
                    ? "same uid " + info.uid + " - shared process OK"
                    : "uid " + info.uid + " != " + Process.myUid() + " - NOT SHARED");
        } catch (PackageManager.NameNotFoundException e) {
            text.append("game     not installed");
        }

        status.setText(text.toString());
        boolean healthy = NativeBridge.AVAILABLE && canOverlay && (!gameInstalled || gameShared);
        status.setTextColor(healthy ? FG : WARN);

        grantButton.setEnabled(!canOverlay);
        grantButton.setText(canOverlay ? "Overlay permission granted" : "Grant overlay permission");
        startButton.setEnabled(canOverlay && NativeBridge.AVAILABLE && !EngineService.running);
        stopButton.setEnabled(EngineService.running);
        gameButton.setEnabled(gameInstalled);

        if (gameInstalled && !gameShared) {
            status.append("\n\nUninstall BOTH apps and reinstall. A package's uid is "
                    + "fixed at install time, so adding sharedUserId to an "
                    + "already-installed package does nothing.");
        }
    }

    // -- actions -------------------------------------------------------------

    private void requestOverlay() {
        startActivity(new Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
                Uri.parse("package:" + getPackageName())));
    }

    private void startEngine() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
                && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS)
                != PackageManager.PERMISSION_GRANTED) {
            // Asked for, never depended on: the foreground service runs either
            // way, the notification and its Stop action just stay invisible.
            // That is why Stop also exists on this screen.
            requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS}, 1);
        }
        EngineService.start(this);
        // Give onCreate a moment before the status block reads `running`.
        status.postDelayed(this::renderStatus, 300);
    }

    private void openGame() {
        Intent intent = getPackageManager().getLaunchIntentForPackage(GAME);
        if (intent == null) {
            Toast.makeText(this, "Dungeon Tap is not installed", Toast.LENGTH_LONG).show();
            return;
        }
        startActivity(intent);
    }

    // -- widgets -------------------------------------------------------------

    private Button addButton(LinearLayout parent, String text, View.OnClickListener listener) {
        Button button = new Button(this);
        button.setText(text);
        button.setAllCaps(false);
        button.setOnClickListener(listener);
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        params.topMargin = dp(10);
        button.setGravity(Gravity.CENTER);
        parent.addView(button, params);
        return button;
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }
}
