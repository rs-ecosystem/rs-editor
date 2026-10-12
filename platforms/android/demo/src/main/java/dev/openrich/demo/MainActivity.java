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
