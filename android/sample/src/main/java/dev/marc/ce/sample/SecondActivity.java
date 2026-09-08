package dev.marc.ce.sample;

import android.app.Activity;
import android.graphics.Color;
import android.os.Bundle;
import android.view.Gravity;
import android.widget.TextView;

/**
 * Somewhere else to be. Switching here and back exercises the overlay's
 * detach/re-attach path: the bubble should follow, and the scan results and
 * address table should survive - they live in the Rust session, not in an
 * Activity.
 */
public final class SecondActivity extends Activity {

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        TextView text = new TextView(this);
        text.setText("Second Activity.\nThe overlay should have followed you here.");
        text.setTextSize(18f);
        text.setTextColor(Color.WHITE);
        text.setGravity(Gravity.CENTER);
        text.setPadding(48, 48, 48, 48);
        setContentView(text);
    }
}
