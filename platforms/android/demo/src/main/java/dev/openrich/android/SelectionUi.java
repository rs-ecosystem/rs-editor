package dev.openrich.android;

import android.content.ActivityNotFoundException;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.graphics.Rect;
import android.net.Uri;
import android.os.Build;
import android.view.ActionMode;
import android.view.HapticFeedbackConstants;
import android.view.Menu;
import android.view.MenuItem;
import android.view.View;
import android.widget.Magnifier;

import org.json.JSONArray;
import org.json.JSONObject;

import java.util.ArrayList;
import java.util.List;

/**
 * The platform side of touch selection: Android's own floating text menu
 * (with "process text" actions such as Translate), its magnifier and its
 * haptics. The Rust view decides when; this class only shows them.
 */
final class SelectionUi extends ActionMode.Callback2 {
    private static final int ID_CUT = 1, ID_COPY = 2, ID_PASTE = 3, ID_SELECT_ALL = 4;
    private static final int ID_PROCESS_TEXT = 100;

    private final OpenRichView view;
    private final Rect contentRect = new Rect();
    private List<String> items = new ArrayList<>();
    private List<ResolveInfo> processText;
    private ActionMode mode;
    private Magnifier magnifier;

    SelectionUi(OpenRichView view) {
        this.view = view;
    }

    /** Applies effects from {@code OpenRichNative.takeEffects}. */
    void apply(JSONArray effects) {
        for (int i = 0; i < effects.length(); i++) {
            JSONObject effect = effects.optJSONObject(i);
            if (effect == null) continue;
            switch (effect.optString("type")) {
                case "haptic":
                    haptic(effect.optString("kind"));
                    break;
                case "menu":
                    JSONObject menu = effect.optJSONObject("menu");
                    if (menu == null) hideMenu(); else showMenu(menu);
                    break;
                case "magnifier":
                    JSONArray at = effect.optJSONArray("at");
                    if (at == null) dismissMagnifier();
                    else showMagnifier((float) at.optDouble(0), (float) at.optDouble(1));
                    break;
                case "showKeyboard":
                    view.showKeyboard();
                    break;
                case "clipboard":
                    setClipboard(effect.optString("text"));
                    break;
                case "openUrl":
                    openUrl(effect.optString("url"));
                    break;
                default:
                    break;
            }
        }
    }

    void release() {
        hideMenu();
        dismissMagnifier();
    }

    // --- haptics -------------------------------------------------------------

    private void haptic(String kind) {
        int constant;
        if ("longPress".equals(kind)) {
            constant = HapticFeedbackConstants.LONG_PRESS;
        } else if (Build.VERSION.SDK_INT >= 27) {
            constant = HapticFeedbackConstants.TEXT_HANDLE_MOVE;
        } else {
            constant = HapticFeedbackConstants.CLOCK_TICK;
        }
        // Follows the user's touch feedback setting.
        view.performHapticFeedback(constant);
    }

    // --- magnifier -----------------------------------------------------------

    private void showMagnifier(float x, float y) {
        if (Build.VERSION.SDK_INT < 28) return;
        if (magnifier == null) {
            magnifier = Build.VERSION.SDK_INT >= 29 ? new Magnifier.Builder(view).build() : new Magnifier(view);
        }
        magnifier.show(x, y);
    }

    private void dismissMagnifier() {
        if (magnifier != null && Build.VERSION.SDK_INT >= 28) magnifier.dismiss();
    }

    // --- floating menu -------------------------------------------------------

    private void showMenu(JSONObject menu) {
        JSONArray rect = menu.optJSONArray("rect");
        if (rect != null) {
            contentRect.set((int) rect.optDouble(0), (int) rect.optDouble(1),
                    (int) Math.ceil(rect.optDouble(2)), (int) Math.ceil(rect.optDouble(3)));
        }
        List<String> next = new ArrayList<>();
        JSONArray list = menu.optJSONArray("items");
        for (int i = 0; list != null && i < list.length(); i++) next.add(list.optString(i));
        items = next;
        if (mode == null) {
            mode = view.startActionMode(this, ActionMode.TYPE_FLOATING);
            if (android.util.Log.isLoggable("OpenRichView", android.util.Log.DEBUG)) {
                android.util.Log.d("OpenRichView", "startActionMode -> " + mode);
            }
        } else {
            mode.invalidate();
            mode.invalidateContentRect();
        }
    }

    private void hideMenu() {
        if (mode != null) {
            ActionMode closing = mode;
            mode = null;
            closing.finish();
        }
    }

    @Override
    public boolean onCreateActionMode(ActionMode mode, Menu menu) {
        populate(menu);
        return true;
    }

    @Override
    public boolean onPrepareActionMode(ActionMode mode, Menu menu) {
        menu.clear();
        populate(menu);
        return true;
    }

    private void populate(Menu menu) {
        int order = 0;
        for (String item : items) {
            switch (item) {
                case "cut": add(menu, ID_CUT, order++, android.R.string.cut); break;
                case "copy": add(menu, ID_COPY, order++, android.R.string.copy); break;
                case "paste": add(menu, ID_PASTE, order++, android.R.string.paste); break;
                case "selectAll": add(menu, ID_SELECT_ALL, order++, android.R.string.selectAll); break;
                default: break;
            }
        }
        // Actions other apps offer on selected text (Translate, search...).
        if (items.contains("copy")) {
            List<ResolveInfo> actions = processTextActions();
            PackageManager pm = view.getContext().getPackageManager();
            for (int i = 0; i < actions.size(); i++) {
                menu.add(Menu.NONE, ID_PROCESS_TEXT + i, 100 + i, actions.get(i).loadLabel(pm))
                        .setShowAsAction(MenuItem.SHOW_AS_ACTION_NEVER);
            }
        }
    }

    private static void add(Menu menu, int id, int order, int title) {
        menu.add(Menu.NONE, id, order, title).setShowAsAction(MenuItem.SHOW_AS_ACTION_IF_ROOM);
    }

    private List<ResolveInfo> processTextActions() {
        if (processText == null) {
            Intent intent = new Intent(Intent.ACTION_PROCESS_TEXT).setType("text/plain");
            processText = view.getContext().getPackageManager().queryIntentActivities(intent, 0);
        }
        return processText;
    }

    @Override
    public boolean onActionItemClicked(ActionMode mode, MenuItem menuItem) {
        int id = menuItem.getItemId();
        if (id >= ID_PROCESS_TEXT) {
            startProcessText(processTextActions().get(id - ID_PROCESS_TEXT));
            hideMenu();
            return true;
        }
        String item;
        switch (id) {
            case ID_CUT: item = "cut"; break;
            case ID_COPY: item = "copy"; break;
            case ID_PASTE: item = "paste"; break;
            case ID_SELECT_ALL: item = "selectAll"; break;
            default: return false;
        }
        String clipboard = "paste".equals(item) ? clipboardText() : null;
        String copied = OpenRichNative.menuAction(view.handle, item, clipboard);
        if (copied != null) setClipboard(copied);
        view.afterInput();
        return true;
    }

    @Override
    public void onDestroyActionMode(ActionMode destroyed) {
        // Still current means the system closed it, not the editor.
        if (mode == destroyed) {
            mode = null;
            OpenRichNative.menuDismissed(view.handle);
        }
    }

    @Override
    public void onGetContentRect(ActionMode mode, View view, Rect outRect) {
        outRect.set(contentRect);
    }

    private void startProcessText(ResolveInfo action) {
        OpenRichView.ImeState state = view.imeState();
        String text = state.text.substring(state.selectionStart, state.selectionEnd);
        Intent intent = new Intent(Intent.ACTION_PROCESS_TEXT)
                .setType("text/plain")
                .setClassName(action.activityInfo.packageName, action.activityInfo.name)
                .putExtra(Intent.EXTRA_PROCESS_TEXT, text)
                .putExtra(Intent.EXTRA_PROCESS_TEXT_READONLY, true)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        try {
            view.getContext().startActivity(intent);
        } catch (ActivityNotFoundException | SecurityException ignored) {
            // The app went away or refuses; nothing to do.
        }
    }

    // --- clipboard and links -------------------------------------------------

    private ClipboardManager clipboard() {
        return view.getContext().getSystemService(ClipboardManager.class);
    }

    private void setClipboard(String text) {
        if (text != null && !text.isEmpty()) {
            clipboard().setPrimaryClip(ClipData.newPlainText("OpenRich", text));
        }
    }

    private String clipboardText() {
        ClipData clip = clipboard().getPrimaryClip();
        if (clip == null || clip.getItemCount() == 0) return null;
        Context context = view.getContext();
        CharSequence text = clip.getItemAt(0).coerceToText(context);
        return text == null ? null : text.toString();
    }

    private void openUrl(String url) {
        try {
            view.getContext().startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url))
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
        } catch (ActivityNotFoundException ignored) {
            // No browser.
        }
    }
}
