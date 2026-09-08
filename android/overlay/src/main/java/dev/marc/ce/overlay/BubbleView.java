package dev.marc.ce.overlay;

import android.annotation.SuppressLint;
import android.content.Context;
import android.graphics.Color;
import android.graphics.drawable.GradientDrawable;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewConfiguration;
import android.view.ViewGroup;
import android.view.WindowManager;
import android.widget.FrameLayout;
import android.widget.TextView;

/**
 * The collapsed handle: a draggable dot that shows scan state and opens the
 * panel on a tap.
 *
 * <p>Drag and tap are told apart by touch slop rather than by a
 * {@code GestureDetector}, so a slow drag never fires a click and a tap never
 * nudges the position.
 */
@SuppressLint("ViewConstructor")
final class BubbleView extends FrameLayout {

    private final OverlayController controller;
    private final TextView label;
    private final int touchSlop;

    private float downRawX, downRawY;
    private int downX, downY;
    private boolean dragging;

    BubbleView(Context context, OverlayController controller) {
        super(context);
        this.controller = controller;
        this.touchSlop = ViewConfiguration.get(context).getScaledTouchSlop();

        int size = controller.dp(48);
        GradientDrawable circle = new GradientDrawable();
        circle.setShape(GradientDrawable.OVAL);
        circle.setColor(Color.argb(230, 24, 26, 32));
        circle.setStroke(controller.dp(2), Color.argb(255, 120, 200, 140));
        setBackground(circle);

        label = new TextView(context);
        label.setText("CE");
        label.setTextColor(Color.argb(255, 190, 235, 200));
        label.setTextSize(13f);
        label.setGravity(Gravity.CENTER);
        addView(label, new LayoutParams(size, size));

        setOnTouchListener(this::onTouchEvent0);
    }

    void setStatus(String text) {
        label.setText(text);
    }

    private boolean onTouchEvent0(View view, MotionEvent event) {
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                downRawX = event.getRawX();
                downRawY = event.getRawY();
                int[] xy = currentPosition();
                downX = xy[0];
                downY = xy[1];
                dragging = false;
                return true;

            case MotionEvent.ACTION_MOVE: {
                float dx = event.getRawX() - downRawX;
                float dy = event.getRawY() - downRawY;
                if (!dragging && Math.hypot(dx, dy) > touchSlop) {
                    dragging = true;
                }
                if (dragging) {
                    moveTo(downX + Math.round(dx), downY + Math.round(dy));
                }
                return true;
            }

            case MotionEvent.ACTION_UP:
                if (!dragging) {
                    controller.toggle();
                }
                return true;

            default:
                return false;
        }
    }

    private int[] currentPosition() {
        ViewGroup.LayoutParams params = getLayoutParams();
        if (params instanceof WindowManager.LayoutParams) {
            WindowManager.LayoutParams wp = (WindowManager.LayoutParams) params;
            return new int[]{wp.x, wp.y};
        }
        return new int[]{Math.round(getTranslationX()), Math.round(getTranslationY())};
    }

    private void moveTo(int x, int y) {
        if (getLayoutParams() instanceof WindowManager.LayoutParams) {
            controller.onBubbleMoved(x, y);
        } else {
            // Content-view fallback: no window to move, so translate instead.
            setTranslationX(x);
            setTranslationY(y);
        }
    }

    /**
     * Fallback for ROMs that refuse a panel window. Only the embedded host has
     * a content view to fall back to.
     */
    void attachToContentView(ViewGroup content, int x, int y) {
        int size = controller.dp(48);
        FrameLayout.LayoutParams params = new FrameLayout.LayoutParams(size, size);
        params.gravity = Gravity.TOP | Gravity.START;
        setTranslationX(x);
        setTranslationY(y);
        content.addView(this, params);
    }

    void detach(WindowManager windowManager) {
        ViewParentSafeRemove.remove(this, windowManager);
    }
}
