package dev.marc.ce.app;

import android.Manifest;
import android.app.Activity;
import android.content.Intent;
import android.content.pm.ApplicationInfo;
import android.content.pm.PackageManager;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
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

import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.List;

import dev.marc.ce.overlay.NativeBridge;

/**
 * The Cheat Engine app's own screen: grant the permission, start the engine,
 * pick a target.
 *
 * <p>The target list is <em>discovered</em>, not hardcoded. Any app declaring
 * the same {@code sharedUserId} lands in this app's uid, and any of those that
 * also declares the same {@code android:process} lands in this app's process -
 * which is the only thing that makes it scannable. So the launcher asks the
 * system who its housemates are instead of naming one, and says plainly which
 * of them actually share the process.
 *
 * <p>That distinction is the entire diagnostic. Same uid but a different
 * process name is the failure mode that produces no error at all: both apps
 * install, both run, and scans simply find nothing.
 */
public final class LauncherActivity extends Activity {

    private static final int BG = Color.rgb(16, 18, 22);
    private static final int CARD = Color.rgb(24, 27, 33);
    private static final int FG = Color.rgb(222, 228, 236);
    private static final int DIM = Color.rgb(140, 150, 165);
    private static final int OK = Color.rgb(120, 200, 140);
    private static final int WARN = Color.rgb(235, 130, 120);

    private TextView status;
    private Button grantButton;
    private Button startButton;
    private Button stopButton;
    private TextView targetsHeading;
    private LinearLayout targetList;

    /** One same-uid package, with everything a row needs to render. */
    private static final class Target {
        String packageName;
        CharSequence label;
        String processName;
        boolean sameProcess;
        Intent launch;
    }

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
        blurb.setText("Scans the process it lives in. Any app declaring the same "
                + "sharedUserId and the same android:process gets put in that "
                + "process by Android - which is why no root is needed, and why "
                + "nothing else on the device is visible.");
        blurb.setTextColor(DIM);
        blurb.setTextSize(13f);
        blurb.setPadding(0, dp(8), 0, dp(16));
        root.addView(blurb);

        status = new TextView(this);
        status.setTypeface(Typeface.MONOSPACE);
        status.setTextSize(12f);
        status.setLineSpacing(dp(2), 1f);
        status.setPadding(dp(12), dp(12), dp(12), dp(12));
        status.setBackgroundColor(CARD);
        root.addView(status);

        grantButton = addButton(root, "Grant overlay permission", v -> requestOverlay());
        startButton = addButton(root, "Start engine", v -> startEngine());
        stopButton = addButton(root, "Stop engine", v -> {
            EngineService.stop(this);
            // onDestroy clears the flag; give it a moment before re-reading.
            status.postDelayed(this::renderStatus, 300);
        });

        targetsHeading = new TextView(this);
        targetsHeading.setTextColor(DIM);
        targetsHeading.setTextSize(11f);
        targetsHeading.setAllCaps(true);
        targetsHeading.setPadding(0, dp(24), 0, dp(10));
        root.addView(targetsHeading);

        targetList = new LinearLayout(this);
        targetList.setOrientation(LinearLayout.VERTICAL);
        root.addView(targetList);

        TextView hint = new TextView(this);
        hint.setText("Tap the CE bubble to open the panel. It stays on screen "
                + "over the target. Force-stopping any app in the shared process "
                + "kills the process, and the scan with it.");
        hint.setTextColor(DIM);
        hint.setTextSize(12f);
        hint.setPadding(0, dp(20), 0, 0);
        root.addView(hint);

        setContentView(scroll);
    }

    @Override
    protected void onResume() {
        super.onResume();
        // canDrawOverlays is known to lag right after the Settings toggle on
        // some ROMs, so it is re-read every time the screen comes back rather
        // than cached. The target list is rebuilt for the same reason: an app
        // can be installed or removed while this screen sits in the background.
        renderStatus();
    }

    // -- targets -------------------------------------------------------------

    /**
     * Every other package sharing this app's uid.
     *
     * <p>{@code getPackagesForUid} on your own uid needs no permission and is
     * not subject to package-visibility filtering - packages sharing a uid are
     * always visible to one another. That is why this app declares no
     * {@code <queries>} entry and no {@code QUERY_ALL_PACKAGES}.
     */
    private List<Target> findTargets() {
        PackageManager pm = getPackageManager();
        List<Target> out = new ArrayList<>();
        String[] packages = pm.getPackagesForUid(Process.myUid());
        if (packages == null) {
            return out;
        }
        String self = getPackageName();
        String ourProcess = getApplicationInfo().processName;

        for (String name : packages) {
            if (self.equals(name)) {
                continue;
            }
            Target t = new Target();
            t.packageName = name;
            try {
                ApplicationInfo info = pm.getApplicationInfo(name, 0);
                t.label = pm.getApplicationLabel(info);
                t.processName = info.processName;
            } catch (PackageManager.NameNotFoundException e) {
                // Shares our uid but cannot be read. List it anyway rather than
                // pretending it is not there.
                t.label = name;
                t.processName = null;
            }
            t.sameProcess = ourProcess != null && ourProcess.equals(t.processName);
            t.launch = pm.getLaunchIntentForPackage(name);
            out.add(t);
        }

        // Scannable ones first, then alphabetical. The list is a menu of what
        // can actually be inspected, so those belong at the top.
        Collections.sort(out, new Comparator<Target>() {
            @Override
            public int compare(Target a, Target b) {
                if (a.sameProcess != b.sameProcess) {
                    return a.sameProcess ? -1 : 1;
                }
                return a.label.toString().compareToIgnoreCase(b.label.toString());
            }
        });
        return out;
    }

    private void renderTargets(List<Target> targets) {
        targetList.removeAllViews();

        if (targets.isEmpty()) {
            TextView empty = new TextView(this);
            empty.setText("No other app shares this uid.\n\n"
                    + "A target must declare, in its own manifest:\n\n"
                    + "    android:sharedUserId=\"" + sharedUserIdGuess() + "\"\n"
                    + "    android:process=\"" + getApplicationInfo().processName + "\"\n\n"
                    + "and be signed with the same certificate. A package's uid is "
                    + "fixed when it is installed, so adding sharedUserId to an "
                    + "already-installed app does nothing - uninstall both and "
                    + "install again.");
            empty.setTextColor(WARN);
            empty.setTextSize(12f);
            empty.setPadding(dp(12), dp(12), dp(12), dp(12));
            empty.setBackgroundColor(CARD);
            targetList.addView(empty);
            return;
        }

        for (Target t : targets) {
            targetList.addView(buildTargetRow(t));
        }
    }

    private View buildTargetRow(final Target t) {
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.VERTICAL);
        row.setPadding(dp(14), dp(12), dp(14), dp(12));

        GradientDrawable bg = new GradientDrawable();
        bg.setColor(CARD);
        bg.setCornerRadius(dp(8));
        bg.setStroke(dp(1), t.sameProcess ? OK : Color.rgb(70, 52, 30));
        row.setBackground(bg);

        TextView label = new TextView(this);
        label.setText(t.label);
        label.setTextColor(t.sameProcess ? FG : DIM);
        label.setTextSize(16f);
        label.setTypeface(null, Typeface.BOLD);
        row.addView(label);

        TextView pkg = new TextView(this);
        pkg.setText(t.packageName);
        pkg.setTypeface(Typeface.MONOSPACE);
        pkg.setTextSize(11f);
        pkg.setTextColor(DIM);
        row.addView(pkg);

        TextView state = new TextView(this);
        state.setTypeface(Typeface.MONOSPACE);
        state.setTextSize(11f);
        state.setPadding(0, dp(6), 0, 0);
        if (t.sameProcess) {
            state.setText("● 同一行程 · 可掃描");
            state.setTextColor(OK);
        } else {
            state.setText("○ 同 uid，但 process = " + t.processName
                    + "\n   不同行程 · 從這裡掃不到");
            state.setTextColor(WARN);
        }
        row.addView(state);

        if (t.launch != null) {
            row.setOnClickListener(v -> startActivity(t.launch));
            row.setClickable(true);
            row.setFocusable(true);
        } else {
            // A service- or library-only package has no launcher entry. It can
            // still be in the shared process, so it belongs in the list - it
            // just cannot be started from here.
            TextView note = new TextView(this);
            note.setText("沒有啟動器入口，無法從這裡開啟");
            note.setTextColor(DIM);
            note.setTextSize(11f);
            note.setPadding(0, dp(4), 0, 0);
            row.addView(note);
        }

        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        params.bottomMargin = dp(8);
        row.setLayoutParams(params);
        return row;
    }

    /**
     * {@code ApplicationInfo} exposes no accessor for the shared user id, so
     * this derives it from the process name, which this project always builds
     * as {@code <sharedUserId>.proc}. Used only as guidance in the empty state.
     */
    private String sharedUserIdGuess() {
        String process = getApplicationInfo().processName;
        if (process == null) {
            return "(unknown)";
        }
        return process.endsWith(".proc")
                ? process.substring(0, process.length() - ".proc".length())
                : process;
    }

    // -- status --------------------------------------------------------------

    private void renderStatus() {
        boolean canOverlay = Settings.canDrawOverlays(this);
        List<Target> targets = findTargets();

        int scannable = 0;
        for (Target t : targets) {
            if (t.sameProcess) {
                scannable++;
            }
        }
        int strays = targets.size() - scannable;

        String text = "package  " + getPackageName() + '\n'
                + "pid      " + Process.myPid() + '\n'
                + "uid      " + Process.myUid() + '\n'
                + "process  " + getApplicationInfo().processName + '\n'
                + "engine   " + (NativeBridge.AVAILABLE
                        ? NativeBridge.nativeVersionLine() : "UNAVAILABLE") + '\n'
                + "overlay  " + (canOverlay ? "granted" : "NOT GRANTED") + '\n'
                + "service  " + (EngineService.running ? "running" : "stopped") + '\n'
                + "targets  " + scannable + " scannable"
                + (strays > 0 ? "  (" + strays + " same uid, other process)" : "");
        status.setText(text);
        status.setTextColor(
                NativeBridge.AVAILABLE && canOverlay && scannable > 0 ? FG : WARN);

        grantButton.setEnabled(!canOverlay);
        grantButton.setText(canOverlay ? "Overlay permission granted" : "Grant overlay permission");
        startButton.setEnabled(canOverlay && NativeBridge.AVAILABLE && !EngineService.running);
        stopButton.setEnabled(EngineService.running);

        targetsHeading.setText(targets.isEmpty()
                ? "同一個 uid 的 App"
                : "同一個 uid 的 App — 點一下啟動");
        renderTargets(targets);
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
