package dev.marc.ce.game;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;

/**
 * Everything the game keeps score of, in one direct {@link ByteBuffer}.
 *
 * <p>Not an ordinary object with ordinary fields, and the reason is the whole
 * point of the exercise. Java fields live on the ART heap, which the
 * concurrent-copying collector <em>moves</em>: an address found by scanning is
 * correct right up until the next GC, after which a freeze on it silently stops
 * working. A direct buffer is native memory - malloc'd once, never relocated -
 * so its address behaves the way a desktop game's does.
 *
 * <p>The fields are also deliberately packed contiguously in one allocation.
 * Once a scan narrows down to {@code gold}, the hex view shows {@code score},
 * {@code wave}, {@code attack} and {@code armor} sitting right beside it at
 * fixed offsets - so working out the rest of the struct is a payoff instead of
 * four more scans.
 *
 * <p>{@link #javaGold} is the control group: the same number in a plain Java
 * field, findable by the same scan and guaranteed to go stale. The Force GC
 * button on the game screen makes that happen on demand.
 */
final class GameState {

    static final int OFF_HP = 0;
    static final int OFF_MAX_HP = 4;
    static final int OFF_GOLD = 8;
    static final int OFF_SCORE = 12;
    static final int OFF_WAVE = 16;
    static final int OFF_ATTACK = 20;
    static final int OFF_ARMOR = 24;
    static final int OFF_POTIONS = 28;
    static final int OFF_MONSTER_HP = 32;
    static final int OFF_MONSTER_MAX_HP = 36;
    static final int OFF_TOTAL_DAMAGE = 40;   // i64, exercises the 8-byte types
    static final int OFF_DROP_RATE = 48;      // f32, exercises the float types

    /**
     * Static and never released, so the mapping stays put for the life of the
     * process and an address in the address table survives leaving and
     * re-entering the game.
     */
    static final ByteBuffer S =
            ByteBuffer.allocateDirect(64).order(ByteOrder.LITTLE_ENDIAN);

    /**
     * The ART-heap decoy: the same number, held in an ordinary object field.
     *
     * <p>An <em>instance</em> field on purpose. Static primitives live in the
     * Class object, which ART keeps in the non-moving space, so a static int
     * would quietly be as stable as the buffer and demonstrate nothing. A
     * regular object is allocated in the region space, which the
     * concurrent-copying collector compacts - so an address found here may stop
     * being that value's address after a GC, which is precisely the limitation
     * worth showing.
     */
    static final class Purse {
        int javaGold;
    }

    static Purse purse = new Purse();

    private GameState() {
    }

    static void reset() {
        S.putInt(OFF_HP, 100);
        S.putInt(OFF_MAX_HP, 100);
        S.putInt(OFF_GOLD, 0);
        S.putInt(OFF_SCORE, 0);
        S.putInt(OFF_WAVE, 1);
        S.putInt(OFF_ATTACK, 3);
        S.putInt(OFF_ARMOR, 0);
        S.putInt(OFF_POTIONS, 1);
        S.putLong(OFF_TOTAL_DAMAGE, 0L);
        S.putFloat(OFF_DROP_RATE, 1.0f);
        spawn();
        purse.javaGold = 0;
    }

    /** Next monster. HP scales with the wave, so gold stays scarce. */
    static void spawn() {
        int hp = 6 + get(OFF_WAVE) * 4;
        S.putInt(OFF_MONSTER_MAX_HP, hp);
        S.putInt(OFF_MONSTER_HP, hp);
    }

    static int get(int offset) {
        return S.getInt(offset);
    }

    static void set(int offset, int value) {
        S.putInt(offset, value);
    }

    static void add(int offset, int delta) {
        S.putInt(offset, S.getInt(offset) + delta);
    }

    static long totalDamage() {
        return S.getLong(OFF_TOTAL_DAMAGE);
    }

    static float dropRate() {
        return S.getFloat(OFF_DROP_RATE);
    }

    /** Kept in step with the native gold so both are the same scan target. */
    static void setGold(int value) {
        set(OFF_GOLD, value);
        purse.javaGold = value;
    }

    static boolean spend(int cost) {
        if (get(OFF_GOLD) < cost) {
            return false;
        }
        setGold(get(OFF_GOLD) - cost);
        return true;
    }
}
