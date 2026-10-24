/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 *
 * Parts of this file are derived from SDL 2's Android project template, which
 * has a different license. Please see vendor/SDL/LICENSE.txt for details.
 */
package org.skymrp.android;

import android.app.ActionBar;
import android.app.AlertDialog;
import android.content.Intent;
import android.graphics.Color;
import android.graphics.drawable.GradientDrawable;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.util.TypedValue;
import android.view.Menu;
import android.view.MenuItem;
import android.view.View;
import android.view.ViewGroup;
import android.widget.FrameLayout;
import android.widget.ImageButton;
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
    private static final int MENU_FULLSCREEN = 2;
    private static final float MIN_SURFACE_HEIGHT_DP = 160;

    private VirtualKeypadView virtualKeypad;
    private VirtualKeypadView.LayoutMode keypadMode = VirtualKeypadView.LayoutMode.COMPACT;
    private CharSequence mrpTitle;
    private boolean keypadVisible = true;
    private boolean fullscreen;
    private int normalSystemUiVisibility;
    private Object fullscreenBackCallback;
    private ImageButton exitFullscreenButton;

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

        super.onCreate(savedInstanceState);

        setUserDataPath();
        ActionBar actionBar = getActionBar();
        if (actionBar != null) {
            actionBar.setDisplayHomeAsUpEnabled(true);
            setTitle(mrpTitle);
        }

        if (mLayout == null || mSurface == null) {
            return;
        }

        insetLayoutBelowActionBar();

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

    private void insetLayoutBelowActionBar() {
        TypedValue actionBarSize = new TypedValue();
        if (getTheme().resolveAttribute(android.R.attr.actionBarSize, actionBarSize, true)) {
            setLayoutTopMargin(
                TypedValue.complexToDimensionPixelSize(
                    actionBarSize.data,
                    getResources().getDisplayMetrics()
                )
            );
        }
    }

    @Override
    public boolean onCreateOptionsMenu(Menu menu) {
        menu.add(Menu.NONE, MENU_FULLSCREEN, Menu.NONE, "Enter fullscreen")
            .setShowAsAction(MenuItem.SHOW_AS_ACTION_NEVER);
        menu.add(Menu.NONE, MENU_KEYPAD, Menu.NONE, "Switch keypad")
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

    @SuppressWarnings("deprecation")
    private void enterFullscreen() {
        if (!fullscreen) {
            normalSystemUiVisibility = getWindow()
                .getDecorView()
                .getSystemUiVisibility();
        }
        fullscreen = true;
        releaseVirtualKeys();
        ActionBar actionBar = getActionBar();
        if (actionBar != null) {
            actionBar.hide();
        }
        setLayoutTopMargin(0);
        getWindow().getDecorView().setSystemUiVisibility(
            View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                | View.SYSTEM_UI_FLAG_FULLSCREEN
                | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
        );
        updateKeypadLayout(mLayout.getHeight());
        addExitFullscreenButton();
        registerFullscreenBackCallback();
    }

    @SuppressWarnings("deprecation")
    private void exitFullscreen() {
        fullscreen = false;
        unregisterFullscreenBackCallback();
        removeExitFullscreenButton();
        getWindow().getDecorView().setSystemUiVisibility(normalSystemUiVisibility);
        ActionBar actionBar = getActionBar();
        if (actionBar != null) {
            actionBar.show();
        }
        insetLayoutBelowActionBar();
        updateKeypadLayout(mLayout.getHeight());
    }

    private void addExitFullscreenButton() {
        if (exitFullscreenButton != null) {
            return;
        }
        exitFullscreenButton = new ImageButton(this);
        exitFullscreenButton.setImageResource(android.R.drawable.ic_menu_revert);
        exitFullscreenButton.setColorFilter(Color.WHITE);
        exitFullscreenButton.setContentDescription("Exit fullscreen");
        exitFullscreenButton.setPadding(dp(12), dp(12), dp(12), dp(12));
        exitFullscreenButton.setElevation(dp(4));
        exitFullscreenButton.setOnClickListener(view -> exitFullscreen());

        GradientDrawable background = new GradientDrawable();
        background.setColor(0x99000000);
        background.setShape(GradientDrawable.OVAL);
        exitFullscreenButton.setBackground(background);

        RelativeLayout.LayoutParams params = new RelativeLayout.LayoutParams(dp(48), dp(48));
        params.addRule(RelativeLayout.ALIGN_PARENT_TOP);
        params.addRule(RelativeLayout.ALIGN_PARENT_END);
        params.setMargins(0, dp(8), dp(8), 0);
        mLayout.addView(exitFullscreenButton, params);
    }

    private void removeExitFullscreenButton() {
        if (exitFullscreenButton == null) {
            return;
        }
        mLayout.removeView(exitFullscreenButton);
        exitFullscreenButton = null;
    }

    private void registerFullscreenBackCallback() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
            && fullscreenBackCallback == null) {
            fullscreenBackCallback = FullscreenBackCallbackApi33.register(this);
        }
    }

    private void unregisterFullscreenBackCallback() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
            && fullscreenBackCallback != null) {
            FullscreenBackCallbackApi33.unregister(this, fullscreenBackCallback);
            fullscreenBackCallback = null;
        }
    }

    private void setLayoutTopMargin(int topMargin) {
        ViewGroup.LayoutParams layoutParams = mLayout.getLayoutParams();
        if (layoutParams instanceof FrameLayout.LayoutParams) {
            FrameLayout.LayoutParams frameParams = (FrameLayout.LayoutParams) layoutParams;
            if (frameParams.topMargin == topMargin) {
                return;
            }
            frameParams.topMargin = topMargin;
            mLayout.setLayoutParams(frameParams);
        }
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
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (!hasFocus) {
            releaseVirtualKeys();
        } else if (fullscreen) {
            enterFullscreen();
        }
    }

    @Override
    public void onBackPressed() {
        if (fullscreen) {
            exitFullscreen();
            return;
        }
        super.onBackPressed();
    }

    @Override
    public boolean onOptionsItemSelected(MenuItem item) {
        if (item.getItemId() == android.R.id.home) {
            finish();
            return true;
        }
        if (item.getItemId() == MENU_KEYPAD) {
            showKeypadDialog();
            return true;
        }
        if (item.getItemId() == MENU_FULLSCREEN) {
            enterFullscreen();
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
            !fullscreen
                && keypadMode != VirtualKeypadView.LayoutMode.NONE
                && keypadVisible;
        virtualKeypad.setVisibility(visible ? View.VISIBLE : View.INVISIBLE);
    }

    private void updateKeypadLayout(int layoutHeight) {
        int availableKeypadHeight = Math.max(
            0,
            layoutHeight - dp(MIN_SURFACE_HEIGHT_DP)
        );
        int actualKeypadHeight =
            fullscreen || keypadMode == VirtualKeypadView.LayoutMode.NONE
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

    private static final class FullscreenBackCallbackApi33 {
        private FullscreenBackCallbackApi33() {
        }

        static Object register(MainActivity activity) {
            android.window.OnBackInvokedCallback callback = activity::exitFullscreen;
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
