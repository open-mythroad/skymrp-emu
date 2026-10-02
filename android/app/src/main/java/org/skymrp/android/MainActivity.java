/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 *
 * Parts of this file are derived from SDL 2's Android project template, which
 * has a different license. Please see vendor/SDL/LICENSE.txt for details.
 */
package org.skymrp.android;

import android.os.Bundle;
import android.os.Environment;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowManager;
import android.widget.RelativeLayout;

import org.libsdl.app.SDLActivity;

/**
 * A wrapper class over SDLActivity
 */

public class MainActivity extends SDLActivity {
    private static final int MRP_SCREEN_WIDTH = 240;
    private static final int MRP_SCREEN_HEIGHT = 320;
    private static final int COMMAND_SET_VIRTUAL_KEYPAD_VISIBLE = COMMAND_USER;

    private VirtualKeypadView virtualKeypad;

    @Override
    protected String[] getLibraries() {
        return new String[]{
            "SDL2",
            "skymrp"
        };
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        setUserDataPath();

        if (mLayout == null || mSurface == null) {
            return;
        }

        virtualKeypad = new VirtualKeypadView(this);
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
            int layoutWidth = right - left;
            int layoutHeight = bottom - top;
            int actualKeypadHeight = Math.max(
                0,
                layoutHeight
                    - Math.round(
                        layoutWidth * MRP_SCREEN_HEIGHT / (float) MRP_SCREEN_WIDTH
                    )
            );
            ViewGroup.LayoutParams params = virtualKeypad.getLayoutParams();
            if (params.height != actualKeypadHeight) {
                params.height = actualKeypadHeight;
                virtualKeypad.setLayoutParams(params);
            }
        });
        hideSystemBars();
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
        if (hasFocus) {
            hideSystemBars();
        } else {
            releaseVirtualKeys();
        }
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
        virtualKeypad.setVisibility(visible ? View.VISIBLE : View.INVISIBLE);
        return true;
    }

    private void hideSystemBars() {
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN);
        getWindow().getDecorView().setSystemUiVisibility(
            View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                | View.SYSTEM_UI_FLAG_FULLSCREEN
                | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
        );
    }

    private void releaseVirtualKeys() {
        if (virtualKeypad != null) {
            virtualKeypad.releaseAllKeys();
        }
    }
}
