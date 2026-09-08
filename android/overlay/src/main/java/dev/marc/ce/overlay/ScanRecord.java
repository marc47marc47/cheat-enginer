package dev.marc.ce.overlay;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.util.ArrayList;
import java.util.List;

/**
 * One row from a packed page: {@code i64 address}, {@code 8 bytes value},
 * {@code i32 valueTypeIndex}, {@code i32 flags}.
 *
 * <p>Value formatting is duplicated from {@code ScanValue::display_value} in
 * Rust. That duplication is deliberate - shipping formatted strings across JNI
 * would cost a String allocation per row - but it does mean the two have to be
 * changed together.
 */
public final class ScanRecord {

    public static final int FLAG_FROZEN = 1;
    public static final int FLAG_FREEZE_ERROR = 1 << 1;

    /** Indices into the engine's ValueType::ALL. */
    public static final int U8 = 0, U16 = 1, U32 = 2, U64 = 3;
    public static final int I8 = 4, I16 = 5, I32 = 6, I64 = 7;
    public static final int F32 = 8, F64 = 9;

    public final long address;
    public final long bits;
    public final int valueType;
    public final int flags;

    private ScanRecord(long address, long bits, int valueType, int flags) {
        this.address = address;
        this.bits = bits;
        this.valueType = valueType;
        this.flags = flags;
    }

    public static List<ScanRecord> decode(byte[] packed) {
        List<ScanRecord> out = new ArrayList<>();
        if (packed == null) {
            return out;
        }
        ByteBuffer buf = ByteBuffer.wrap(packed).order(ByteOrder.LITTLE_ENDIAN);
        while (buf.remaining() >= NativeBridge.RECORD_SIZE) {
            out.add(new ScanRecord(buf.getLong(), buf.getLong(), buf.getInt(), buf.getInt()));
        }
        return out;
    }

    public boolean isFrozen() {
        return (flags & FLAG_FROZEN) != 0;
    }

    public boolean hasFreezeError() {
        return (flags & FLAG_FREEZE_ERROR) != 0;
    }

    public String addressText() {
        return "0x" + Long.toHexString(address).toUpperCase();
    }

    /** The value, rendered the way the engine would render it. */
    public String valueText() {
        switch (valueType) {
            case U8:
                return Long.toString(bits & 0xFFL);
            case U16:
                return Long.toString(bits & 0xFFFFL);
            case U32:
                return Long.toString(bits & 0xFFFFFFFFL);
            case U64:
                return Long.toUnsignedString(bits);
            case I8:
                return Long.toString((byte) bits);
            case I16:
                return Long.toString((short) bits);
            case I32:
                return Long.toString((int) bits);
            case I64:
                return Long.toString(bits);
            case F32:
                return sixPlaces(Float.intBitsToFloat((int) bits));
            case F64:
                return sixPlaces(Double.longBitsToDouble(bits));
            default:
                return "?";
        }
    }

    /** Six decimal places and no trimming, matching Rust's {@code {v:.6}}. */
    private static String sixPlaces(double v) {
        return String.format(java.util.Locale.US, "%.6f", v);
    }
}
