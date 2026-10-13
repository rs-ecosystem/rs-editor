package dev.openrich.demo;

import android.app.Activity;
import android.content.res.Configuration;
import android.graphics.Insets;
import android.os.Bundle;
import android.view.Window;
import android.view.WindowInsets;
import android.widget.FrameLayout;

import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;

import dev.openrich.android.OpenRichView;

/** Shows the editor full screen, clear of the system bars and keyboard. */
public final class MainActivity extends Activity {
    private OpenRichView editor;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        requestWindowFeature(Window.FEATURE_NO_TITLE);
        FrameLayout root = new FrameLayout(this);
        editor = new OpenRichView(this, readAsset("sample.json"), isDark());
        root.addView(editor);
        // Android 15 draws edge to edge: keep the editor inside the system
        // bars and above the keyboard.
        root.setOnApplyWindowInsetsListener((view, insets) -> {
            Insets bars = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.ime()
                    | WindowInsets.Type.displayCutout());
            view.setPadding(bars.left, bars.top, bars.right, bars.bottom);
            return WindowInsets.CONSUMED;
        });
        setContentView(root);
        editor.requestFocus();
        registerAccessibilityProbe();
    }

    /**
     * Debug aid: `adb shell am broadcast -a dev.openrich.demo.A11Y_PROBE --ei granularity 4
     * --ez forward true` performs what TalkBack does when moving through text and logs the
     * result under the tag OpenRichProbe.
     */
    private void registerAccessibilityProbe() {
        if ((getApplicationInfo().flags & android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE) == 0) return;
        android.content.BroadcastReceiver probe = new android.content.BroadcastReceiver() {
            @Override
            public void onReceive(android.content.Context context, android.content.Intent intent) {
                android.view.accessibility.AccessibilityNodeProvider provider = editor.getAccessibilityNodeProvider();
                if (provider == null) {
                    android.util.Log.d("OpenRichProbe", "no node provider");
                    return;
                }
                int host = android.view.View.NO_ID; // AccessibilityNodeProvider.HOST_VIEW_ID
                android.view.accessibility.AccessibilityNodeInfo info = provider.createAccessibilityNodeInfo(host);
                if (info == null) {
                    android.util.Log.d("OpenRichProbe", "no host node");
                    return;
                }
                android.util.Log.d("OpenRichProbe", "host: class=" + info.getClassName()
                        + " editable=" + info.isEditable() + " focused=" + info.isFocused()
                        + " a11yFocused=" + info.isAccessibilityFocused()
                        + " granularities=" + info.getMovementGranularities()
                        + " selection=" + info.getTextSelectionStart() + ".." + info.getTextSelectionEnd()
                        + " textLength=" + (info.getText() == null ? -1 : info.getText().length())
                        + " actions=" + info.getActionList());
                int granularity = intent.getIntExtra("granularity", 4);
                boolean forward = intent.getBooleanExtra("forward", true);
                Bundle args = new Bundle();
                args.putInt(android.view.accessibility.AccessibilityNodeInfo.ACTION_ARGUMENT_MOVEMENT_GRANULARITY_INT, granularity);
                args.putBoolean(android.view.accessibility.AccessibilityNodeInfo.ACTION_ARGUMENT_EXTEND_SELECTION_BOOLEAN, false);
                int action = forward
                        ? android.view.accessibility.AccessibilityNodeInfo.ACTION_NEXT_AT_MOVEMENT_GRANULARITY
                        : android.view.accessibility.AccessibilityNodeInfo.ACTION_PREVIOUS_AT_MOVEMENT_GRANULARITY;
                boolean result = provider.performAction(host, action, args);
                android.view.accessibility.AccessibilityNodeInfo after = provider.createAccessibilityNodeInfo(host);
                android.util.Log.d("OpenRichProbe", "performAction(" + (forward ? "next" : "previous")
                        + ", granularity " + granularity + ") = " + result + ", selection now "
                        + (after == null ? "?" : after.getTextSelectionStart() + ".." + after.getTextSelectionEnd()));
            }
        };
        android.content.IntentFilter filter = new android.content.IntentFilter("dev.openrich.demo.A11Y_PROBE");
        if (android.os.Build.VERSION.SDK_INT >= 33) {
            registerReceiver(probe, filter, android.content.Context.RECEIVER_EXPORTED);
        } else {
            registerReceiver(probe, filter);
        }
    }

    @Override
    public void onConfigurationChanged(Configuration config) {
        super.onConfigurationChanged(config);
        editor.setDark(isDark());
    }

    @Override
    protected void onDestroy() {
        editor.release();
        super.onDestroy();
    }

    private boolean isDark() {
        int mode = getResources().getConfiguration().uiMode & Configuration.UI_MODE_NIGHT_MASK;
        return mode == Configuration.UI_MODE_NIGHT_YES;
    }

    private String readAsset(String name) {
        try (InputStream in = getAssets().open(name)) {
            return new String(in.readAllBytes(), StandardCharsets.UTF_8);
        } catch (IOException e) {
            return "{\"type\":\"doc\",\"content\":[{\"type\":\"paragraph\"}]}";
        }
    }
}
