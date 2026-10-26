/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 *
 * Parts of this file are derived from SDL 2's Android project template, which
 * has a different license. Please see vendor/SDL/LICENSE.txt for details.
 */
package org.skymrp.android;

import android.app.AlertDialog;
import android.content.Intent;
import android.content.SharedPreferences;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.util.TypedValue;
import android.view.Menu;
import android.view.MenuItem;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowManager;
import android.widget.RelativeLayout;

import org.libsdl.app.SDLActivity;

/**
 * A wrapper class over SDLActivity
 */

public class MainActivity extends SDLActivity {
    public static final String EXTRA_MRP_PATH = "org.skymrp.android.extra.MRP_PATH";
    public static final String EXTRA_MRP_TITLE = "org.skymrp.android.extra.MRP_TITLE";
    public static final String EXTRA_SCREEN_RESOLUTION =
        "org.skymrp.android.extra.SCREEN_RESOLUTION";
    public static final String EXTRA_FULL_VM_ENABLED =
        "org.skymrp.android.extra.FULL_VM_ENABLED";
    public static final String EXTRA_KEYPAD_MODE =
        "org.skymrp.android.extra.KEYPAD_MODE";

    private static final int COMMAND_SET_VIRTUAL_KEYPAD_VISIBLE = COMMAND_USER;
    private static final int MENU_KEYPAD = 1;
    private static final int MENU_EXIT = 2;
    private static final float MIN_SURFACE_HEIGHT_DP = 160;

    private VirtualKeypadView virtualKeypad;
    private VirtualKeypadView.LayoutMode keypadMode = VirtualKeypadView.LayoutMode.COMPACT;
    private CharSequence mrpTitle;
    private boolean statusBarEnabled;
    private boolean keypadVisible = true;
    private Object menuBackCallback;

    @Override
    protected String[] getLibraries() {
        return new String[]{
            "SDL2",
            "skymrp"
        };
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        Intent intent = getIntent();
        mrpTitle = intent.getStringExtra(EXTRA_MRP_TITLE);
        keypadMode = VirtualKeypadView.LayoutMode.fromValue(
            intent.getStringExtra(EXTRA_KEYPAD_MODE)
        );
        SharedPreferences preferences = getSharedPreferences(
            EmulatorSettings.PREFERENCES,
            MODE_PRIVATE
        );
        statusBarEnabled = preferences.getBoolean(
            EmulatorSettings.STATUS_BAR_ENABLED,
            EmulatorSettings.DEFAULT_STATUS_BAR_ENABLED
        );

        super.onCreate(savedInstanceState);

        setUserDataPath();

        if (mLayout == null || mSurface == null) {
            return;
        }

        applyWindowSettings();
        updateMenuBackCallback();

        virtualKeypad = new VirtualKeypadView(this);
        virtualKeypad.setLayoutMode(keypadMode);
        virtualKeypad.setId(View.generateViewId());

        RelativeLayout.LayoutParams keypadParams = new RelativeLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            0
        );
        keypadParams.addRule(RelativeLayout.ALIGN_PARENT_BOTTOM);

        mLayout.addView(virtualKeypad, keypadParams);

        RelativeLayout.LayoutParams surfaceParams = new RelativeLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.MATCH_PARENT
        );
        surfaceParams.addRule(RelativeLayout.ABOVE, virtualKeypad.getId());
        mSurface.setLayoutParams(surfaceParams);

        mLayout.addOnLayoutChangeListener((view, left, top, right, bottom,
                                           oldLeft, oldTop, oldRight, oldBottom) -> {
            updateKeypadLayout(bottom - top);
        });
    }

    @Override
    public void setTitle(CharSequence title) {
        super.setTitle(mrpTitle != null ? mrpTitle : title);
    }

    @Override
    public boolean onCreateOptionsMenu(Menu menu) {
        menu.add(Menu.NONE, MENU_KEYPAD, Menu.NONE, "Switch keypad")
            .setShowAsAction(MenuItem.SHOW_AS_ACTION_NEVER);
        menu.add(Menu.NONE, MENU_EXIT, Menu.NONE, "Exit")
            .setShowAsAction(MenuItem.SHOW_AS_ACTION_NEVER);
        return true;
    }

    private void showKeypadDialog() {
        VirtualKeypadView.LayoutMode[] modes = {
            VirtualKeypadView.LayoutMode.FULL,
            VirtualKeypadView.LayoutMode.COMPACT,
            VirtualKeypadView.LayoutMode.PHONE,
            VirtualKeypadView.LayoutMode.NONE,
        };
        String[] labels = {"Full", "Compact", "Phone", "None"};
        int selected = 0;
        for (int index = 0; index < modes.length; index++) {
            if (modes[index] == keypadMode) {
                selected = index;
                break;
            }
        }

        new AlertDialog.Builder(this)
            .setTitle("Switch keypad")
            .setSingleChoiceItems(labels, selected, (dialog, position) -> {
                dialog.dismiss();
                switchKeypad(modes[position]);
            })
            .setNegativeButton(android.R.string.cancel, null)
            .show();
    }

    private void switchKeypad(VirtualKeypadView.LayoutMode mode) {
        if (mode == keypadMode) {
            return;
        }
        keypadMode = mode;
        virtualKeypad.setLayoutMode(mode);
        getSharedPreferences(EmulatorSettings.PREFERENCES, MODE_PRIVATE)
            .edit()
            .putString(EmulatorSettings.KEYPAD_MODE, mode.value)
            .apply();
        getIntent().putExtra(EXTRA_KEYPAD_MODE, mode.value);
        updateKeypadLayout(mLayout.getHeight());
    }

    private void updateMenuBackCallback() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
            && menuBackCallback == null) {
            menuBackCallback = MenuBackCallbackApi33.register(this);
        }
    }

    private void applyWindowSettings() {
        if (statusBarEnabled) {
            getWindow().clearFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN);
            getWindow().addFlags(WindowManager.LayoutParams.FLAG_FORCE_NOT_FULLSCREEN);
        } else {
            getWindow().clearFlags(WindowManager.LayoutParams.FLAG_FORCE_NOT_FULLSCREEN);
            getWindow().addFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN);
        }
    }

    private void showRuntimeMenu() {
        releaseVirtualKeys();
        openOptionsMenu();
    }

    private int dp(float value) {
        return Math.round(
            TypedValue.applyDimension(
                TypedValue.COMPLEX_UNIT_DIP,
                value,
                getResources().getDisplayMetrics()
            )
        );
    }

    @Override
    protected String[] getArguments() {
        Intent intent = getIntent();
        String mrpPath = intent.getStringExtra(EXTRA_MRP_PATH);
        String resolution = intent.getStringExtra(EXTRA_SCREEN_RESOLUTION);
        if (resolution == null) {
            resolution = EmulatorSettings.DEFAULT_SCREEN_RESOLUTION;
        }
        String runtime = intent.getBooleanExtra(EXTRA_FULL_VM_ENABLED, false)
            ? "full"
            : "mini";

        if (mrpPath == null) {
            return new String[]{
                "--screen-size=" + resolution,
                "--runtime=" + runtime
            };
        }
        return new String[]{
            mrpPath,
            "--screen-size=" + resolution,
            "--runtime=" + runtime
        };
    }

    @SuppressWarnings("deprecation")
    private void setUserDataPath() {
        nativeSetenv(
            "SKYMRP_USER_DATA_PATH",
            Environment.getExternalStorageDirectory().getAbsolutePath()
        );
    }

    @Override
    protected void onPause() {
        releaseVirtualKeys();
        super.onPause();
    }

    @Override
    protected void onDestroy() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
            && menuBackCallback != null) {
            MenuBackCallbackApi33.unregister(this, menuBackCallback);
            menuBackCallback = null;
        }
        super.onDestroy();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (!hasFocus) {
            releaseVirtualKeys();
        } else {
            applyWindowSettings();
        }
    }

    @Override
    public void onBackPressed() {
        showRuntimeMenu();
    }

    @Override
    public boolean onOptionsItemSelected(MenuItem item) {
        if (item.getItemId() == MENU_KEYPAD) {
            showKeypadDialog();
            return true;
        }
        if (item.getItemId() == MENU_EXIT) {
            finish();
            return true;
        }
        return super.onOptionsItemSelected(item);
    }

    @Override
    protected boolean onUnhandledMessage(int command, Object param) {
        if (command != COMMAND_SET_VIRTUAL_KEYPAD_VISIBLE) {
            return false;
        }
        if (virtualKeypad == null || !(param instanceof Integer)) {
            return true;
        }

        boolean visible = ((Integer) param) != 0;
        if (!visible) {
            releaseVirtualKeys();
        }
        keypadVisible = visible;
        updateVirtualKeypadVisibility();
        return true;
    }

    private void updateVirtualKeypadVisibility() {
        boolean visible =
            keypadMode != VirtualKeypadView.LayoutMode.NONE
                && keypadVisible;
        virtualKeypad.setVisibility(visible ? View.VISIBLE : View.INVISIBLE);
    }

    private void updateKeypadLayout(int layoutHeight) {
        int availableKeypadHeight = Math.max(
            0,
            layoutHeight - dp(MIN_SURFACE_HEIGHT_DP)
        );
        int actualKeypadHeight =
            keypadMode == VirtualKeypadView.LayoutMode.NONE
            ? 0
            : Math.min(virtualKeypad.getPreferredHeight(), availableKeypadHeight);
        ViewGroup.LayoutParams params = virtualKeypad.getLayoutParams();
        if (params.height != actualKeypadHeight) {
            params.height = actualKeypadHeight;
            virtualKeypad.setLayoutParams(params);
        }
        updateVirtualKeypadVisibility();
    }

    private void releaseVirtualKeys() {
        if (virtualKeypad != null) {
            virtualKeypad.releaseAllKeys();
        }
    }

    private static final class MenuBackCallbackApi33 {
        private MenuBackCallbackApi33() {
        }

        static Object register(MainActivity activity) {
            android.window.OnBackInvokedCallback callback = activity::showRuntimeMenu;
            activity.getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT,
                callback
            );
            return callback;
        }

        static void unregister(MainActivity activity, Object callback) {
            activity.getOnBackInvokedDispatcher().unregisterOnBackInvokedCallback(
                (android.window.OnBackInvokedCallback) callback
            );
        }
    }
}
