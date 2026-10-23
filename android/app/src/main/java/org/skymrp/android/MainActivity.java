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
import android.content.Intent;
import android.os.Bundle;
import android.os.Environment;
import android.util.TypedValue;
import android.view.MenuItem;
import android.view.View;
import android.view.ViewGroup;
import android.widget.FrameLayout;
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

    private static final int DEFAULT_SCREEN_WIDTH = 240;
    private static final int DEFAULT_SCREEN_HEIGHT = 320;
    private static final int COMMAND_SET_VIRTUAL_KEYPAD_VISIBLE = COMMAND_USER;

    private VirtualKeypadView virtualKeypad;
    private CharSequence mrpTitle;
    private int screenWidth = DEFAULT_SCREEN_WIDTH;
    private int screenHeight = DEFAULT_SCREEN_HEIGHT;

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
        parseScreenResolution(intent.getStringExtra(EXTRA_SCREEN_RESOLUTION));

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
                        layoutWidth * screenHeight / (float) screenWidth
                    )
            );
            ViewGroup.LayoutParams params = virtualKeypad.getLayoutParams();
            if (params.height != actualKeypadHeight) {
                params.height = actualKeypadHeight;
                virtualKeypad.setLayoutParams(params);
            }
        });
    }

    @Override
    public void setTitle(CharSequence title) {
        super.setTitle(mrpTitle != null ? mrpTitle : title);
    }

    private void insetLayoutBelowActionBar() {
        TypedValue actionBarSize = new TypedValue();
        if (!getTheme().resolveAttribute(android.R.attr.actionBarSize, actionBarSize, true)) {
            return;
        }
        int topMargin = TypedValue.complexToDimensionPixelSize(
            actionBarSize.data,
            getResources().getDisplayMetrics()
        );

        ViewGroup.LayoutParams layoutParams = mLayout.getLayoutParams();
        if (layoutParams instanceof FrameLayout.LayoutParams) {
            FrameLayout.LayoutParams frameParams = (FrameLayout.LayoutParams) layoutParams;
            frameParams.topMargin = topMargin;
            mLayout.setLayoutParams(frameParams);
        }
    }

    @Override
    protected String[] getArguments() {
        Intent intent = getIntent();
        String mrpPath = intent.getStringExtra(EXTRA_MRP_PATH);
        String resolution = intent.getStringExtra(EXTRA_SCREEN_RESOLUTION);
        if (resolution == null) {
            resolution = DEFAULT_SCREEN_WIDTH + "x" + DEFAULT_SCREEN_HEIGHT;
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
        }
    }

    @Override
    public boolean onOptionsItemSelected(MenuItem item) {
        if (item.getItemId() == android.R.id.home) {
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
        virtualKeypad.setVisibility(visible ? View.VISIBLE : View.INVISIBLE);
        return true;
    }

    private void parseScreenResolution(String resolution) {
        if (resolution == null) {
            return;
        }
        String[] dimensions = resolution.split("x", 2);
        if (dimensions.length != 2) {
            return;
        }
        try {
            int width = Integer.parseInt(dimensions[0]);
            int height = Integer.parseInt(dimensions[1]);
            if (width > 0 && height > 0) {
                screenWidth = width;
                screenHeight = height;
            }
        } catch (NumberFormatException ignored) {
        }
    }

    private void releaseVirtualKeys() {
        if (virtualKeypad != null) {
            virtualKeypad.releaseAllKeys();
        }
    }
}
