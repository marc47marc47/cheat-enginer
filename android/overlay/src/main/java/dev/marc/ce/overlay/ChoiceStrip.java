package dev.marc.ce.overlay;

import android.annotation.SuppressLint;
import android.content.Context;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.view.Gravity;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.TextView;

/**
 * A single-select row of chips, used where a {@link android.widget.Spinner}
 * would be the obvious choice.
 *
 * <p>It is not a style preference. A Spinner opens its list in a
 * {@code ListPopupWindow}, which takes its token from the anchor's window and
 * defaults to {@code TYPE_APPLICATION_PANEL}. Anchored inside a
 * {@code TYPE_APPLICATION_OVERLAY} window that is the classic
 * {@code BadTokenException: token null is not valid; is your activity
 * running?}, and Spinner exposes no public way to change the popup's window
 * type. Drawing the choices inside the panel adds no window at all, so it works
 * identically under both window hosts - and on a phone-sized panel a scrolling
 * strip beats a dropdown for ten value types anyway.
 */
@SuppressLint("ViewConstructor")
final class ChoiceStrip extends HorizontalScrollView {

    private static final int DIM = Color.argb(255, 140, 150, 165);
    private static final int ACCENT = Color.argb(255, 120, 200, 140);
    private static final int OFF = Color.argb(255, 90, 96, 106);

    private final LinearLayout row;
    private final int pad;
    private String[] items = new String[0];
    private int selected;

    ChoiceStrip(Context context, OverlayController controller) {
        super(context);
        this.pad = controller.dp(6);
        setHorizontalScrollBarEnabled(false);
        row = new LinearLayout(context);
        row.setOrientation(LinearLayout.HORIZONTAL);
        addView(row);
    }

    void setItems(String[] values, int initial) {
        items = values != null ? values : new String[0];
        selected = initial < items.length ? initial : 0;
        row.removeAllViews();
        for (int i = 0; i < items.length; i++) {
            final int index = i;
            TextView chip = new TextView(getContext());
            chip.setText(items[i]);
            chip.setTextSize(12f);
            chip.setGravity(Gravity.CENTER);
            chip.setPadding(pad * 2, pad, pad * 2, pad);
            chip.setOnClickListener(v -> {
                if (isEnabled()) {
                    setSelection(index);
                }
            });
            LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.WRAP_CONTENT,
                    LinearLayout.LayoutParams.WRAP_CONTENT);
            lp.rightMargin = pad;
            row.addView(chip, lp);
        }
        paint();
    }

    int getSelectedItemPosition() {
        return selected;
    }

    void setSelection(int index) {
        if (index < 0 || index >= items.length) {
            return;
        }
        selected = index;
        paint();
    }

    @Override
    public void setEnabled(boolean enabled) {
        super.setEnabled(enabled);
        paint();
    }

    private void paint() {
        for (int i = 0; i < row.getChildCount(); i++) {
            TextView chip = (TextView) row.getChildAt(i);
            boolean on = i == selected;
            chip.setTextColor(!isEnabled() ? OFF : (on ? Color.argb(255, 16, 20, 24) : DIM));
            chip.setTypeface(null, on ? Typeface.BOLD : Typeface.NORMAL);
            GradientDrawable bg = new GradientDrawable();
            bg.setCornerRadius(pad * 2f);
            if (on) {
                bg.setColor(isEnabled() ? ACCENT : OFF);
            } else {
                bg.setColor(Color.TRANSPARENT);
                bg.setStroke(Math.max(1, pad / 3), isEnabled() ? DIM : OFF);
            }
            chip.setBackground(bg);
        }
        setAlpha(isEnabled() ? 1f : 0.6f);
    }
}
