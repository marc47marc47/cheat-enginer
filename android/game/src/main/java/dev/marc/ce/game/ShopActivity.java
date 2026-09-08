package dev.marc.ce.game;

import android.app.Activity;
import android.graphics.Color;
import android.graphics.Typeface;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.view.WindowInsets;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;
import android.widget.Toast;

/**
 * Three things to buy, all priced so that honest play cannot afford them for a
 * long time. That is the point: the shop is the payoff for finding gold in
 * memory, and it makes the write visible immediately - every button lights up
 * at once.
 */
public final class ShopActivity extends Activity {

    private static final int BG = Color.rgb(16, 18, 22);
    private static final int FG = Color.rgb(226, 232, 240);
    private static final int DIM = Color.rgb(140, 150, 165);
    private static final int GOLD = Color.rgb(232, 196, 96);

    private static final int SWORD_COST = 60;
    private static final int SHIELD_COST = 75;
    private static final int POTION_COST = 30;

    private TextView purse;
    private Button sword;
    private Button shield;
    private Button potion;

    private final Handler handler = new Handler(Looper.getMainLooper());

    /** Same reason as the game screen: an outside write has to become visible. */
    private final Runnable tick = new Runnable() {
        @Override
        public void run() {
            render();
            handler.postDelayed(this, 200);
        }
    };

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(BG);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            root.setOnApplyWindowInsetsListener((v, insets) -> {
                android.graphics.Insets bars = insets.getInsets(WindowInsets.Type.systemBars());
                v.setPadding(dp(20), bars.top + dp(20), dp(20), bars.bottom + dp(20));
                return insets;
            });
        } else {
            root.setPadding(dp(20), dp(40), dp(20), dp(20));
        }

        TextView title = new TextView(this);
        title.setText("Shop");
        title.setTextColor(GOLD);
        title.setTextSize(22f);
        title.setTypeface(null, Typeface.BOLD);
        root.addView(title);

        purse = new TextView(this);
        purse.setTypeface(Typeface.MONOSPACE);
        purse.setTextSize(14f);
        purse.setPadding(0, dp(8), 0, dp(16));
        root.addView(purse);

        sword = addButton(root, v -> buy(SWORD_COST, () -> {
            GameState.add(GameState.OFF_ATTACK, 2);
            return "Sword bought   attack " + GameState.get(GameState.OFF_ATTACK);
        }));
        shield = addButton(root, v -> buy(SHIELD_COST, () -> {
            GameState.add(GameState.OFF_ARMOR, 1);
            return "Shield bought   armor " + GameState.get(GameState.OFF_ARMOR);
        }));
        potion = addButton(root, v -> buy(POTION_COST, () -> {
            GameState.add(GameState.OFF_POTIONS, 1);
            return "Potion bought   " + GameState.get(GameState.OFF_POTIONS) + " in bag";
        }));

        TextView note = new TextView(this);
        note.setTextColor(DIM);
        note.setTextSize(11f);
        note.setPadding(0, dp(16), 0, 0);
        note.setText("Buying an upgrade also changes attack or armor, which sit"
                + " four and eight bytes after gold in the same allocation. Once"
                + " gold is found, the hex view has the rest.");
        root.addView(note);

        setContentView(root);
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

    private interface Purchase {
        String apply();
    }

    private void buy(int cost, Purchase purchase) {
        if (!GameState.spend(cost)) {
            Toast.makeText(this, "Not enough gold", Toast.LENGTH_SHORT).show();
            return;
        }
        Toast.makeText(this, purchase.apply(), Toast.LENGTH_SHORT).show();
        render();
    }

    private void render() {
        int gold = GameState.get(GameState.OFF_GOLD);
        purse.setText("gold " + gold);
        purse.setTextColor(GOLD);
        label(sword, "Sword   +2 attack", SWORD_COST, gold);
        label(shield, "Shield  +1 armor", SHIELD_COST, gold);
        label(potion, "Potion  heals 40", POTION_COST, gold);
    }

    private void label(Button button, String text, int cost, int gold) {
        button.setText(text + "        " + cost + "g");
        button.setEnabled(gold >= cost);
        button.setTextColor(gold >= cost ? FG : DIM);
    }

    private Button addButton(LinearLayout parent, android.view.View.OnClickListener listener) {
        Button button = new Button(this);
        button.setAllCaps(false);
        button.setTextSize(14f);
        button.setOnClickListener(listener);
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        params.topMargin = dp(10);
        parent.addView(button, params);
        return button;
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }
}
