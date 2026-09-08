package dev.marc.ce.game;

import android.app.Activity;
import android.content.Intent;
import android.graphics.Color;
import android.graphics.Typeface;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.os.Process;
import android.view.Gravity;
import android.view.View;
import android.view.WindowInsets;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;
import android.widget.Toast;

import java.util.Random;

/**
 * Dungeon Tap.
 *
 * <p>Tap the monster to hurt it, miss and it gets a free swing, kill it for
 * gold, spend gold on gear that lets you survive the next wave. Gold is
 * deliberately scarce, which is what makes finding it in memory worth doing.
 *
 * <p>This app knows nothing about the scanner: no dependency, no native
 * library, no permission, no source file that mentions it. The only thing it
 * does on the scanner's behalf is declare the same {@code sharedUserId} and
 * {@code android:process} in its manifest, which is what puts both packages in
 * one OS process - and that is the whole reason no root is needed.
 */
public final class GameActivity extends Activity implements ArenaView.Listener {

    private static final int BG = Color.rgb(16, 18, 22);
    private static final int FG = Color.rgb(226, 232, 240);
    private static final int DIM = Color.rgb(140, 150, 165);
    private static final int GOLD = Color.rgb(232, 196, 96);
    private static final int WARN = Color.rgb(235, 130, 120);

    /** How often the monster swings back. */
    private static final long ATTACK_MS = 1500;

    /**
     * How often the HUD is repainted regardless of what the game did.
     *
     * <p>Without this the numbers only change when the game itself changes
     * them, so a value written into memory from outside stays invisible until
     * the next tap - which is precisely the moment a memory editor is supposed
     * to be demonstrating.
     */
    private static final long HUD_MS = 200;

    private final Handler handler = new Handler(Looper.getMainLooper());
    private final Random random = new Random();

    private TextView hud;
    private ArenaView arena;
    private Button potionButton;
    private boolean dead;

    private final Runnable hudTick = new Runnable() {
        @Override
        public void run() {
            renderHud();
            handler.postDelayed(this, HUD_MS);
        }
    };

    private final Runnable monsterTurn = new Runnable() {
        @Override
        public void run() {
            if (!dead) {
                takeHit(monsterDamage());
                handler.postDelayed(this, ATTACK_MS);
            }
        }
    };

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        if (GameState.get(GameState.OFF_MAX_HP) == 0) {
            GameState.reset();
        }

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(BG);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            root.setOnApplyWindowInsetsListener((v, insets) -> {
                android.graphics.Insets bars = insets.getInsets(WindowInsets.Type.systemBars());
                v.setPadding(dp(14), bars.top + dp(10), dp(14), bars.bottom + dp(10));
                return insets;
            });
        } else {
            root.setPadding(dp(14), dp(30), dp(14), dp(10));
        }

        TextView title = new TextView(this);
        title.setText("Dungeon Tap");
        title.setTextColor(GOLD);
        title.setTextSize(20f);
        title.setTypeface(null, Typeface.BOLD);
        root.addView(title);

        hud = new TextView(this);
        hud.setTypeface(Typeface.MONOSPACE);
        hud.setTextSize(13f);
        hud.setLineSpacing(dp(2), 1f);
        hud.setPadding(0, dp(6), 0, dp(8));
        root.addView(hud);

        arena = new ArenaView(this);
        arena.setListener(this);
        // A fixed height, on purpose. The scanner's panel occupies the bottom of
        // the screen, and the playfield has to stay tappable while it is open -
        // "change a value in the game, then narrow the scan" is the entire
        // workflow being demonstrated.
        root.addView(arena, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, dp(200)));

        LinearLayout buttons = new LinearLayout(this);
        buttons.setOrientation(LinearLayout.HORIZONTAL);
        buttons.setPadding(0, dp(8), 0, 0);
        potionButton = addButton(buttons, "Potion", v -> drinkPotion());
        addButton(buttons, "Shop", v -> startActivity(new Intent(this, ShopActivity.class)));
        addButton(buttons, "Force GC", v -> forceGc());
        root.addView(buttons);

        TextView hint = new TextView(this);
        hint.setTextColor(DIM);
        hint.setTextSize(11f);
        hint.setPadding(0, dp(10), 0, 0);
        hint.setText("gold and score live in a direct ByteBuffer - native memory,"
                + " stable address. javaGold is the same number in an ordinary"
                + " object field: just as findable, but ART's collector may move"
                + " it, and then the address is no longer that value's address.");
        root.addView(hint);

        setContentView(root);
    }

    @Override
    protected void onResume() {
        super.onResume();
        arena.start();
        if (!dead) {
            handler.postDelayed(monsterTurn, ATTACK_MS);
        }
        handler.post(hudTick);
    }

    @Override
    protected void onPause() {
        super.onPause();
        arena.stop();
        handler.removeCallbacks(monsterTurn);
        handler.removeCallbacks(hudTick);
    }

    // -- combat --------------------------------------------------------------

    @Override
    public void onHit() {
        if (dead) {
            revive();
            return;
        }
        int damage = GameState.get(GameState.OFF_ATTACK) + random.nextInt(3);
        GameState.add(GameState.OFF_MONSTER_HP, -damage);
        GameState.S.putLong(GameState.OFF_TOTAL_DAMAGE, GameState.totalDamage() + damage);
        GameState.add(GameState.OFF_SCORE, damage);

        if (GameState.get(GameState.OFF_MONSTER_HP) <= 0) {
            int wave = GameState.get(GameState.OFF_WAVE);
            // An irregular reward, so "gold went up by exactly N" is not a
            // shortcut and the scan genuinely has to be narrowed.
            int reward = Math.round((7 + random.nextInt(9) + wave * 2) * GameState.dropRate());
            GameState.setGold(GameState.get(GameState.OFF_GOLD) + reward);
            GameState.add(GameState.OFF_SCORE, 10 * wave);
            GameState.set(GameState.OFF_WAVE, wave + 1);
            GameState.spawn();
            arena.respawn();
            toast("Wave " + wave + " cleared   +" + reward + " gold");
        }
        renderHud();
    }

    @Override
    public void onMiss() {
        if (dead) {
            revive();
            return;
        }
        // Missing has to cost something, or tapping wildly is the best strategy.
        takeHit(Math.max(1, monsterDamage() / 2));
    }

    private int monsterDamage() {
        int wave = GameState.get(GameState.OFF_WAVE);
        int armor = GameState.get(GameState.OFF_ARMOR);
        return Math.max(1, 3 + wave - armor + random.nextInt(3) - 1);
    }

    private void takeHit(int damage) {
        GameState.add(GameState.OFF_HP, -damage);
        if (GameState.get(GameState.OFF_HP) <= 0) {
            GameState.set(GameState.OFF_HP, 0);
            dead = true;
            handler.removeCallbacks(monsterTurn);
            toast("You died. Tap the arena to start over.");
        }
        renderHud();
    }

    /**
     * Death costs progress but not the purse. Gold is the value this whole
     * thing exists to demonstrate finding; wiping it every time the monster
     * wins would make the walkthrough impossible to follow.
     */
    private void revive() {
        GameState.set(GameState.OFF_HP, GameState.get(GameState.OFF_MAX_HP));
        GameState.set(GameState.OFF_WAVE, 1);
        GameState.spawn();
        arena.respawn();
        dead = false;
        handler.postDelayed(monsterTurn, ATTACK_MS);
        renderHud();
    }

    private void drinkPotion() {
        if (GameState.get(GameState.OFF_POTIONS) <= 0) {
            toast("No potions. Buy one in the shop.");
            return;
        }
        GameState.add(GameState.OFF_POTIONS, -1);
        GameState.set(GameState.OFF_HP, Math.min(GameState.get(GameState.OFF_MAX_HP),
                GameState.get(GameState.OFF_HP) + 40));
        renderHud();
    }

    /**
     * Makes the standing limitation visible instead of mysterious: after this,
     * an address found in the Java purse may no longer be that value's address,
     * while the same number in the direct buffer has not moved at all.
     */
    private void forceGc() {
        // Churn first. A collection with nothing to reclaim has little reason
        // to compact, and compaction is the part that moves the purse.
        Object[] churn = new Object[4096];
        for (int i = 0; i < churn.length; i++) {
            churn[i] = new byte[2048];
        }
        churn = null;
        System.gc();
        System.runFinalization();
        System.gc();
        toast("GC forced - a freeze on javaGold may point at nothing now");
    }

    // -- hud -----------------------------------------------------------------

    private void renderHud() {
        int hp = GameState.get(GameState.OFF_HP);
        int maxHp = GameState.get(GameState.OFF_MAX_HP);
        String text = "HP    " + bar(hp, maxHp) + ' ' + hp + '/' + maxHp + '\n'
                + "gold  " + GameState.get(GameState.OFF_GOLD)
                + "   javaGold " + GameState.purse.javaGold + '\n'
                + "score " + GameState.get(GameState.OFF_SCORE)
                + "   wave " + GameState.get(GameState.OFF_WAVE) + '\n'
                + "atk " + GameState.get(GameState.OFF_ATTACK)
                + "  armor " + GameState.get(GameState.OFF_ARMOR)
                + "  potions " + GameState.get(GameState.OFF_POTIONS) + '\n'
                + "pid " + Process.myPid() + "  uid " + Process.myUid();
        hud.setText(text);
        hud.setTextColor(dead || hp * 4 < maxHp ? WARN : FG);

        potionButton.setText("Potion (" + GameState.get(GameState.OFF_POTIONS) + ")");
        potionButton.setTextColor(GameState.get(GameState.OFF_POTIONS) > 0 ? FG : DIM);
    }

    private static String bar(int value, int max) {
        int cells = 10;
        int filled = max <= 0 ? 0 : Math.max(0, Math.min(cells, value * cells / max));
        StringBuilder out = new StringBuilder("[");
        for (int i = 0; i < cells; i++) {
            out.append(i < filled ? '#' : '.');
        }
        return out.append(']').toString();
    }

    // -- widgets -------------------------------------------------------------

    private Button addButton(LinearLayout parent, String text, View.OnClickListener listener) {
        Button button = new Button(this);
        button.setText(text);
        button.setAllCaps(false);
        button.setTextSize(13f);
        button.setGravity(Gravity.CENTER);
        button.setOnClickListener(listener);
        parent.addView(button, new LinearLayout.LayoutParams(
                0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f));
        return button;
    }

    private void toast(String message) {
        Toast.makeText(this, message, Toast.LENGTH_SHORT).show();
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }
}
