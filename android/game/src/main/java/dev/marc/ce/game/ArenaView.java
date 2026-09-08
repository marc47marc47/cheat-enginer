package dev.marc.ce.game;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.RectF;
import android.view.MotionEvent;
import android.view.View;

import java.util.Random;

/**
 * The playfield: one monster drifting around, hit by tapping it.
 *
 * <p>Deliberately a plain {@link View} with an invalidate loop rather than a
 * SurfaceView or any drawing library - the game exists to be a realistic
 * scanning target, and every dependency it took on would be one the target
 * would not normally have.
 */
final class ArenaView extends View {

    interface Listener {
        /** A tap landed on the monster. */
        void onHit();

        /** A tap missed - the monster gets a free swing. */
        void onMiss();
    }

    private static final long FRAME_MS = 16;
    private static final int BG = Color.rgb(22, 25, 31);

    private final Paint fill = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint stroke = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint text = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Random random = new Random();
    private final RectF bar = new RectF();

    private Listener listener;
    private float x, y, vx, vy, radius;
    private float flash;
    private boolean running;

    private final Runnable frame = new Runnable() {
        @Override
        public void run() {
            step();
            invalidate();
            if (running) {
                postDelayed(this, FRAME_MS);
            }
        }
    };

    ArenaView(Context context) {
        super(context);
        setBackgroundColor(BG);
        stroke.setStyle(Paint.Style.STROKE);
        text.setColor(Color.rgb(230, 236, 244));
        text.setTextAlign(Paint.Align.CENTER);
    }

    void setListener(Listener listener) {
        this.listener = listener;
    }

    void start() {
        if (running) {
            return;
        }
        running = true;
        post(frame);
    }

    void stop() {
        running = false;
        removeCallbacks(frame);
    }

    /** Places a fresh monster and picks a new drift direction. */
    void respawn() {
        float density = getResources().getDisplayMetrics().density;
        radius = 34 * density;
        x = Math.max(radius, getWidth() / 2f);
        y = Math.max(radius + barClearance(), getHeight() / 2f);
        // Speed grows with the wave, so later waves are genuinely harder to tap
        // rather than just being damage sponges.
        float speed = (1.4f + 0.35f * GameState.get(GameState.OFF_WAVE)) * density;
        double angle = random.nextDouble() * Math.PI * 2;
        vx = (float) Math.cos(angle) * speed;
        vy = (float) Math.sin(angle) * speed;
        flash = 0f;
    }

    /** Room above the monster for its HP bar, so it never clips off the top. */
    private float barClearance() {
        return 6 * getResources().getDisplayMetrics().density * 3.4f;
    }

    private void step() {
        if (getWidth() == 0 || radius == 0) {
            return;
        }
        x += vx;
        y += vy;
        float top = radius + barClearance();
        if (x < radius) {
            x = radius;
            vx = -vx;
        } else if (x > getWidth() - radius) {
            x = getWidth() - radius;
            vx = -vx;
        }
        if (y < top) {
            y = top;
            vy = -vy;
        } else if (y > getHeight() - radius) {
            y = getHeight() - radius;
            vy = -vy;
        }
        if (flash > 0f) {
            flash = Math.max(0f, flash - 0.08f);
        }
    }

    @Override
    protected void onSizeChanged(int w, int h, int oldw, int oldh) {
        super.onSizeChanged(w, h, oldw, oldh);
        if (radius == 0) {
            respawn();
        }
    }

    @Override
    public boolean onTouchEvent(MotionEvent event) {
        if (event.getActionMasked() != MotionEvent.ACTION_DOWN || listener == null) {
            return true;
        }
        float dx = event.getX() - x;
        float dy = event.getY() - y;
        if (dx * dx + dy * dy <= radius * radius) {
            flash = 1f;
            listener.onHit();
        } else {
            listener.onMiss();
        }
        invalidate();
        return true;
    }

    @Override
    protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);
        if (radius == 0) {
            return;
        }
        float density = getResources().getDisplayMetrics().density;

        int hp = GameState.get(GameState.OFF_MONSTER_HP);
        int max = Math.max(1, GameState.get(GameState.OFF_MONSTER_MAX_HP));

        int body = Color.rgb(
                (int) (150 + 100 * flash),
                (int) (70 - 40 * flash),
                (int) (90 - 40 * flash));
        fill.setColor(body);
        canvas.drawCircle(x, y, radius, fill);

        stroke.setColor(Color.rgb(240, 190, 120));
        stroke.setStrokeWidth(2 * density);
        canvas.drawCircle(x, y, radius, stroke);

        // Eyes, so it reads as a creature and not a dot.
        fill.setColor(Color.rgb(250, 240, 220));
        canvas.drawCircle(x - radius * 0.32f, y - radius * 0.18f, radius * 0.16f, fill);
        canvas.drawCircle(x + radius * 0.32f, y - radius * 0.18f, radius * 0.16f, fill);

        // HP bar riding above it.
        float barWidth = radius * 2f;
        float barHeight = 6 * density;
        bar.set(x - barWidth / 2, y - radius - barHeight * 2.4f,
                x + barWidth / 2, y - radius - barHeight * 1.4f);
        fill.setColor(Color.rgb(60, 64, 74));
        canvas.drawRoundRect(bar, barHeight, barHeight, fill);
        bar.right = bar.left + barWidth * Math.max(0f, hp) / max;
        fill.setColor(Color.rgb(210, 90, 90));
        canvas.drawRoundRect(bar, barHeight, barHeight, fill);

        text.setTextSize(13 * density);
        canvas.drawText("W" + GameState.get(GameState.OFF_WAVE), x, y + radius * 0.42f, text);
    }
}
