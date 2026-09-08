package dev.marc.ce.sample;

import android.app.Activity;
import android.content.Intent;
import android.graphics.Color;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.view.Gravity;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;

/**
 * A target to practise on. Deliberately holds the same counter twice.
 *
 * <p><b>Native counter</b> - a direct {@link ByteBuffer}, so the value lives in
 * a plain {@code malloc} allocation at a fixed address. This is the one the
 * scan-narrow-freeze walkthrough uses, and the one that behaves the way a
 * desktop game does.
 *
 * <p><b>Java counter</b> - an ordinary {@code int} field on the ART heap. ART's
 * concurrent-copying collector <em>moves</em> objects, so an address found for
 * this one goes stale after a GC and a freeze on it quietly stops working. That
 * is not a bug in the scanner; it is the standing limitation of scanning a
 * managed heap, and the "Force GC" button is here to make it visible rather
 * than mysterious.
 *
 * <p>Note what this file does <em>not</em> contain: any reference to the
 * overlay. It installs itself.
 */
public final class MainActivity extends Activity {

    private final ByteBuffer nativeCounter =
            ByteBuffer.allocateDirect(4).order(ByteOrder.LITTLE_ENDIAN);

    private int javaCounter = 100;

    private TextView nativeView;
    private TextView javaView;
    private final Handler handler = new Handler(Looper.getMainLooper());

    private final Runnable tick = new Runnable() {
        @Override
        public void run() {
            render();
            handler.postDelayed(this, 250);
        }
    };

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        nativeCounter.putInt(0, 100);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        // Take the initial focus, or the ScrollView scrolls the first Button to
        // the top on launch and the counter above it is never visible.
        root.setFocusableInTouchMode(true);
        root.setPadding(48, 24, 48, 48);
        root.setGravity(Gravity.CENTER_HORIZONTAL);

        nativeView = big("");
        javaView = big("");
        root.addView(caption("Native counter (direct ByteBuffer)"));
        root.addView(nativeView);
        root.addView(button("-10", v -> add(-10)));
        root.addView(button("+10", v -> add(10)));

        root.addView(caption("Java counter (ART heap - moves on GC)"));
        root.addView(javaView);
        root.addView(button("Java -10", v -> {
            javaCounter -= 10;
            render();
        }));
        root.addView(button("Force GC (watch the Java one go stale)", v -> System.gc()));

        root.addView(button("Open second Activity",
                v -> startActivity(new Intent(this, SecondActivity.class))));

        // Scrollable, because the overlay panel takes the bottom half of the
        // screen and the counters must stay reachable while it is open.
        android.widget.ScrollView scroller = new android.widget.ScrollView(this);
        scroller.addView(root);

        // Android 15 lays every window out edge to edge for targetSdk 35 and
        // ignores setDecorFitsSystemWindows, so the top of the content ends up
        // under the status bar. Pad by the real insets instead.
        scroller.setOnApplyWindowInsetsListener((v, insets) -> {
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.R) {
                android.graphics.Insets bars =
                        insets.getInsets(android.view.WindowInsets.Type.systemBars());
                v.setPadding(0, bars.top, 0, bars.bottom);
            }
            return insets;
        });

        setContentView(scroller);
        render();
    }

    @Override
    protected void onResume() {
        super.onResume();
        handler.post(tick);
    }

    @Override
    protected void onPause() {
        super.onPause();
        handler.removeCallbacks(tick);
    }

    private void add(int delta) {
        nativeCounter.putInt(0, nativeCounter.getInt(0) + delta);
        render();
    }

    private void render() {
        nativeView.setText(String.valueOf(nativeCounter.getInt(0)));
        javaView.setText(String.valueOf(javaCounter));
    }

    private TextView big(String text) {
        TextView view = new TextView(this);
        view.setText(text);
        view.setTextSize(36f);
        view.setTextColor(Color.WHITE);
        view.setGravity(Gravity.CENTER);
        return view;
    }

    private TextView caption(String text) {
        TextView view = new TextView(this);
        view.setText(text);
        view.setTextSize(13f);
        view.setPadding(0, 32, 0, 0);
        view.setTextColor(Color.LTGRAY);
        return view;
    }

    private Button button(String text, android.view.View.OnClickListener listener) {
        Button button = new Button(this);
        button.setText(text);
        button.setAllCaps(false);
        button.setOnClickListener(listener);
        return button;
    }
}
