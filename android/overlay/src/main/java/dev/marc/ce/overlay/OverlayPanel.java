package dev.marc.ce.overlay;

import android.annotation.SuppressLint;
import android.content.Context;
import android.graphics.Color;
import android.graphics.Typeface;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.text.InputType;
import android.view.Gravity;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.widget.AdapterView;
import android.widget.BaseAdapter;
import android.widget.Button;
import android.widget.CheckBox;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ListView;
import android.widget.ProgressBar;
import android.widget.TextView;

import java.util.ArrayList;
import java.util.List;

/**
 * The expanded overlay: scan configuration, results, the address table and a
 * hex view, built entirely from framework widgets.
 *
 * <p>No XML and no AndroidX. The library is dropped into arbitrary debug APKs,
 * so every resource it declares is a merge conflict waiting to happen and every
 * runtime dependency is a version it could force on its host.
 *
 * <p>State lives in the Rust session, not here: this class holds selected tab,
 * spinner indices and text buffers, all of which are cheap to lose when an
 * Activity is recreated.
 */
@SuppressLint("ViewConstructor")
final class OverlayPanel extends LinearLayout {

    private static final int POLL_MS = 250;
    private static final int PAGE = 200;

    private static final int BG = Color.argb(242, 18, 20, 25);
    private static final int FG = Color.argb(255, 222, 228, 236);
    private static final int DIM = Color.argb(255, 140, 150, 165);
    private static final int ACCENT = Color.argb(255, 120, 200, 140);
    private static final int WARN = Color.argb(255, 235, 130, 120);

    private final OverlayController controller;
    private final long session;
    private final Handler handler = new Handler(Looper.getMainLooper());

    private final LinearLayout tabStrip;
    private final LinearLayout body;
    private final TextView statusLine;
    private final List<TextView> tabLabels = new ArrayList<>();
    private int currentTab = 0;

    // Scan tab widgets, kept so the poll can read them without a lookup.
    private ChoiceStrip valueTypeChoice;
    private ChoiceStrip scanTypeChoice;
    private EditText valueInput;
    private ProgressBar progress;

    private ListView resultList;
    private ResultAdapter resultAdapter;
    private ListView tableList;
    private TableAdapter tableAdapter;
    private EditText hexAddress;
    private TextView hexDump;

    private final Runnable poll = new Runnable() {
        @Override
        public void run() {
            refresh();
            handler.postDelayed(this, POLL_MS);
        }
    };

    OverlayPanel(Context context, OverlayController controller) {
        super(context);
        this.controller = controller;
        this.session = controller.session();

        setOrientation(VERTICAL);
        setBackgroundColor(BG);
        final int pad = controller.dp(8);
        setPadding(pad, pad, pad, pad);

        // The panel is anchored to the bottom edge. Under targetSdk 35 a window
        // lays out edge to edge, so without this the bottom row of buttons sits
        // underneath the navigation bar and cannot be tapped. Harmless in
        // embedded mode, where FLAG_LAYOUT_INSET_DECOR already inset the window
        // and this reports zero.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            setOnApplyWindowInsetsListener((v, insets) -> {
                int bottom = insets.getInsets(WindowInsets.Type.systemBars()).bottom;
                v.setPadding(pad, pad, pad, pad + bottom);
                return insets;
            });
        }

        tabStrip = new LinearLayout(context);
        tabStrip.setOrientation(HORIZONTAL);
        addView(tabStrip, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT));
        addTab("Scan", 0);
        addTab("Results", 1);
        addTab("Address", 2);
        addTab("Hex", 3);

        body = new LinearLayout(context);
        body.setOrientation(VERTICAL);
        addView(body, new LayoutParams(LayoutParams.MATCH_PARENT, controller.dp(300)));

        statusLine = label(context, "", DIM);
        addView(statusLine);

        selectTab(0);
    }

    // -- chrome --------------------------------------------------------------

    private void addTab(String title, int index) {
        TextView tab = label(getContext(), title, DIM);
        tab.setPadding(controller.dp(12), controller.dp(6), controller.dp(12), controller.dp(6));
        tab.setOnClickListener(v -> selectTab(index));
        tabLabels.add(tab);
        tabStrip.addView(tab);
    }

    private void selectTab(int index) {
        currentTab = index;
        for (int i = 0; i < tabLabels.size(); i++) {
            TextView tab = tabLabels.get(i);
            tab.setTextColor(i == index ? ACCENT : DIM);
            tab.setTypeface(null, i == index ? Typeface.BOLD : Typeface.NORMAL);
        }
        body.removeAllViews();
        switch (index) {
            case 0:
                body.addView(buildScanTab());
                break;
            case 1:
                body.addView(buildResultsTab());
                break;
            case 2:
                body.addView(buildAddressTab());
                break;
            default:
                body.addView(buildHexTab());
                break;
        }
        refresh();
    }

    // -- tab 1: scan ---------------------------------------------------------

    private View buildScanTab() {
        Context context = getContext();
        LinearLayout root = column(context);

        valueTypeChoice = choice(NativeBridge.nativeValueTypeLabels(), 2);
        scanTypeChoice = choice(NativeBridge.nativeScanTypeLabels(), 0);
        root.addView(valueTypeChoice);
        root.addView(scanTypeChoice);

        valueInput = new EditText(context);
        valueInput.setHint("value");
        valueInput.setTextColor(FG);
        valueInput.setHintTextColor(DIM);
        valueInput.setSingleLine(true);
        valueInput.setInputType(InputType.TYPE_CLASS_NUMBER
                | InputType.TYPE_NUMBER_FLAG_SIGNED
                | InputType.TYPE_NUMBER_FLAG_DECIMAL);
        root.addView(valueInput);

        LinearLayout buttons = new LinearLayout(context);
        buttons.setOrientation(HORIZONTAL);
        buttons.addView(button(context, "New Scan", v -> startScan(true)), weighted());
        buttons.addView(button(context, "Next Scan", v -> startScan(false)), weighted());
        buttons.addView(button(context, "Reset", v -> {
            NativeBridge.nativeResetScan(session);
            refresh();
        }), weighted());
        buttons.addView(button(context, "Stop", v -> NativeBridge.nativeCancelScan(session)), weighted());
        root.addView(buttons);

        progress = new ProgressBar(context, null, android.R.attr.progressBarStyleHorizontal);
        progress.setMax(100);
        root.addView(progress);

        return root;
    }

    private void startScan(boolean restart) {
        String text = valueInput.getText().toString();
        boolean accepted = NativeBridge.nativeStartScan(
                session,
                valueTypeChoice.getSelectedItemPosition(),
                scanTypeChoice.getSelectedItemPosition(),
                text,
                restart);
        if (!accepted) {
            showError();
        }
        refresh();
    }

    // -- tab 2: results ------------------------------------------------------

    private View buildResultsTab() {
        Context context = getContext();
        LinearLayout root = column(context);
        resultList = new ListView(context);
        resultAdapter = new ResultAdapter();
        resultList.setAdapter(resultAdapter);
        resultList.setOnItemClickListener((AdapterView<?> parent, View view, int pos, long id) -> {
            ScanRecord record = resultAdapter.getItem(pos);
            if (record != null) {
                NativeBridge.nativeTableAdd(session, record.address, record.valueType,
                        "scan " + record.addressText());
                NativeBridge.nativeTableSave(session, controller.tablePath());
                selectTab(2);
            }
        });
        root.addView(resultList, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
        return root;
    }

    // -- tab 3: address table ------------------------------------------------

    private View buildAddressTab() {
        Context context = getContext();
        LinearLayout root = column(context);
        tableList = new ListView(context);
        // Without this a ListView keeps focus to itself and the EditText inside
        // a row can never be focused, so the value is not editable at all.
        tableList.setItemsCanFocus(true);
        tableList.setDescendantFocusability(FOCUS_AFTER_DESCENDANTS);
        tableAdapter = new TableAdapter();
        tableList.setAdapter(tableAdapter);
        root.addView(tableList, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
        return root;
    }

    // -- tab 4: hex ----------------------------------------------------------

    private View buildHexTab() {
        Context context = getContext();
        LinearLayout root = column(context);

        LinearLayout bar = new LinearLayout(context);
        bar.setOrientation(HORIZONTAL);
        hexAddress = new EditText(context);
        hexAddress.setHint("0x...");
        hexAddress.setTextColor(FG);
        hexAddress.setHintTextColor(DIM);
        hexAddress.setSingleLine(true);
        bar.addView(hexAddress, weighted());
        bar.addView(button(context, "Go", v -> refresh()));
        root.addView(bar);

        hexDump = label(context, "", FG);
        hexDump.setTypeface(Typeface.MONOSPACE);
        hexDump.setTextSize(11f);
        root.addView(hexDump);
        return root;
    }

    private void refreshHex() {
        if (hexDump == null) {
            return;
        }
        long address = parseAddress(hexAddress.getText().toString());
        if (address == 0) {
            hexDump.setText("enter an address");
            return;
        }
        byte[] bytes = NativeBridge.nativeReadBytes(session, address, 128);
        if (bytes == null) {
            hexDump.setText("unreadable");
            return;
        }
        StringBuilder out = new StringBuilder();
        for (int row = 0; row < bytes.length; row += 16) {
            out.append(String.format("%016X  ", address + row));
            StringBuilder ascii = new StringBuilder();
            for (int i = row; i < row + 16 && i < bytes.length; i++) {
                out.append(String.format("%02X ", bytes[i]));
                char c = (char) (bytes[i] & 0xFF);
                ascii.append(c >= 32 && c < 127 ? c : '.');
            }
            out.append(' ').append(ascii).append('\n');
        }
        hexDump.setText(out.toString());
    }

    private static long parseAddress(String text) {
        String s = text.trim();
        if (s.startsWith("0x") || s.startsWith("0X")) {
            s = s.substring(2);
        }
        try {
            return Long.parseUnsignedLong(s, 16);
        } catch (NumberFormatException e) {
            return 0;
        }
    }

    // -- polling -------------------------------------------------------------

    void onShown() {
        handler.post(poll);
    }

    void onHidden() {
        handler.removeCallbacks(poll);
    }

    private void refresh() {
        long[] status = NativeBridge.nativeScanStatus(session);
        if (status != null && status.length >= 5) {
            boolean running = status[0] == 1;
            int pct = status[2] > 0 ? (int) (status[1] * 100 / status[2]) : (running ? 0 : 100);
            if (progress != null) {
                progress.setProgress(pct);
            }
            // Changing the value type mid-narrow would make next_scan
            // reinterpret the existing addresses at a different width - no
            // crash, just quietly wrong results. Lock it once a scan exists.
            if (valueTypeChoice != null) {
                boolean scanned = status[0] != 0;
                valueTypeChoice.setEnabled(!scanned);
            }
            String suffix = status[4] != 0 ? "+ (capped)" : "";
            statusLine.setText(running
                    ? "scanning " + pct + "%  " + status[3] + " hits" + suffix
                    : status[3] + " results" + suffix);
            statusLine.setTextColor(running ? ACCENT : DIM);
        }
        showError();

        switch (currentTab) {
            case 1:
                if (resultAdapter != null) {
                    resultAdapter.reload();
                }
                break;
            case 2:
                if (tableAdapter != null) {
                    tableAdapter.reload();
                }
                break;
            case 3:
                refreshHex();
                break;
            default:
                break;
        }
    }

    private void showError() {
        String error = NativeBridge.nativeLastError(session);
        if (error != null) {
            statusLine.setText(error);
            statusLine.setTextColor(WARN);
        }
    }

    /** Collapse on back rather than letting a focusable window eat the key. */
    @Override
    public boolean dispatchKeyEvent(KeyEvent event) {
        if (event.getKeyCode() == KeyEvent.KEYCODE_BACK
                && event.getAction() == KeyEvent.ACTION_UP) {
            controller.collapse();
            return true;
        }
        return super.dispatchKeyEvent(event);
    }

    // -- adapters ------------------------------------------------------------

    private final class ResultAdapter extends BaseAdapter {
        private List<ScanRecord> rows = new ArrayList<>();

        void reload() {
            rows = ScanRecord.decode(NativeBridge.nativeResultsPage(session, 0, PAGE));
            notifyDataSetChanged();
        }

        @Override
        public int getCount() {
            return rows.size();
        }

        @Override
        public ScanRecord getItem(int position) {
            return position < rows.size() ? rows.get(position) : null;
        }

        @Override
        public long getItemId(int position) {
            return position;
        }

        @Override
        public View getView(int position, View convertView, ViewGroup parent) {
            TextView row = convertView instanceof TextView
                    ? (TextView) convertView
                    : label(getContext(), "", FG);
            ScanRecord record = rows.get(position);
            row.setTypeface(Typeface.MONOSPACE);
            row.setTextSize(12f);
            row.setText(record.addressText() + "   " + record.valueText());
            return row;
        }
    }

    /**
     * The address table.
     *
     * <p>Rows are recycled and, crucially, a row that currently has focus is
     * left alone. The poll runs four times a second; rebuilding the row views
     * on every tick would wipe whatever the user was halfway through typing and
     * take the cursor with it, which makes editing a value impossible.
     */
    private final class TableAdapter extends BaseAdapter {
        private List<ScanRecord> rows = new ArrayList<>();
        private String[] descriptions = new String[0];

        void reload() {
            rows = ScanRecord.decode(NativeBridge.nativeTablePage(session));
            String[] d = NativeBridge.nativeTableDescriptions(session);
            descriptions = d != null ? d : new String[0];
            notifyDataSetChanged();
        }

        @Override
        public int getCount() {
            return rows.size();
        }

        @Override
        public ScanRecord getItem(int position) {
            return rows.get(position);
        }

        @Override
        public long getItemId(int position) {
            return position;
        }

        @Override
        public View getView(int position, View convertView, ViewGroup parent) {
            Row row = convertView instanceof LinearLayout && convertView.getTag() instanceof Row
                    ? (Row) convertView.getTag()
                    : new Row(getContext());
            row.bind(position, rows.get(position),
                    position < descriptions.length ? descriptions[position] : "");
            return row.view;
        }
    }

    /** One recycled address-table row. */
    private final class Row {
        final LinearLayout view;
        final TextView text;
        final EditText value;
        final CheckBox freeze;
        final Button delete;

        /** Which table entry this row currently shows; the listeners read it. */
        int position;

        Row(Context context) {
            view = new LinearLayout(context);
            view.setOrientation(HORIZONTAL);
            view.setGravity(Gravity.CENTER_VERTICAL);
            view.setTag(this);

            text = label(context, "", FG);
            text.setTypeface(Typeface.MONOSPACE);
            text.setTextSize(11f);
            view.addView(text, weighted());

            value = new EditText(context);
            value.setTextColor(FG);
            value.setSingleLine(true);
            value.setWidth(controller.dp(90));
            value.setImeOptions(android.view.inputmethod.EditorInfo.IME_ACTION_DONE);
            value.setInputType(InputType.TYPE_CLASS_NUMBER
                    | InputType.TYPE_NUMBER_FLAG_SIGNED
                    | InputType.TYPE_NUMBER_FLAG_DECIMAL);
            value.setOnEditorActionListener((v, actionId, event) -> {
                if (!NativeBridge.nativeTableSetValue(session, position, v.getText().toString())) {
                    showError();
                }
                v.clearFocus();
                return true;
            });
            view.addView(value);

            freeze = new CheckBox(context);
            freeze.setText("F");
            freeze.setTextColor(DIM);
            freeze.setOnClickListener(v -> {
                if (!NativeBridge.nativeTableToggleFreeze(session, position)) {
                    showError();
                }
                NativeBridge.nativeTableSave(session, controller.tablePath());
            });
            view.addView(freeze);

            delete = button(context, "x", v -> {
                NativeBridge.nativeTableRemove(session, position);
                NativeBridge.nativeTableSave(session, controller.tablePath());
                if (tableAdapter != null) {
                    tableAdapter.reload();
                }
            });
            view.addView(delete);
        }

        void bind(int position, ScanRecord record, String description) {
            this.position = position;
            text.setText(description + "  " + record.addressText());
            text.setTextColor(record.hasFreezeError() ? WARN : FG);
            // Leave a field the user is editing exactly as they left it.
            if (!value.hasFocus()) {
                value.setText(record.valueText());
            }
            freeze.setChecked(record.isFrozen());
        }
    }

    // -- tiny widget helpers -------------------------------------------------

    private LinearLayout column(Context context) {
        LinearLayout layout = new LinearLayout(context);
        layout.setOrientation(VERTICAL);
        return layout;
    }

    private static TextView label(Context context, String text, int color) {
        TextView view = new TextView(context);
        view.setText(text);
        view.setTextColor(color);
        view.setTextSize(13f);
        return view;
    }

    private Button button(Context context, String text, OnClickListener listener) {
        Button button = new Button(context);
        button.setText(text);
        button.setAllCaps(false);
        button.setTextSize(12f);
        button.setOnClickListener(listener);
        return button;
    }

    private ChoiceStrip choice(String[] items, int selected) {
        ChoiceStrip strip = new ChoiceStrip(getContext(), controller);
        strip.setItems(items, selected);
        return strip;
    }

    private static LayoutParams weighted() {
        LayoutParams params = new LayoutParams(0, LayoutParams.WRAP_CONTENT, 1f);
        return params;
    }
}
