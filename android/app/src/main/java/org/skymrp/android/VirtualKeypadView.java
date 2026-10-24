/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
package org.skymrp.android;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.RectF;
import android.util.AttributeSet;
import android.util.SparseIntArray;
import android.view.HapticFeedbackConstants;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.View;

import org.libsdl.app.SDLActivity;

final class VirtualKeypadView extends View {
    private static final int PHONE_COLUMN_COUNT = 3;
    private static final int COMPACT_COLUMN_COUNT = 6;
    private static final int FULL_ROW_COUNT = 7;
    private static final int PHONE_ROW_COUNT = 5;
    private static final float COMPACT_MAX_KEY_WIDTH_DP = 96;
    private static final float COMPACT_MAX_KEY_HEIGHT_DP = 56;
    private static final float COMPACT_GROUP_GAP_DP = 16;
    private static final float KEY_GAP_DP = 5;
    private static final float VERTICAL_PADDING_DP = 7;
    private static final float FULL_HEIGHT_DP = 336;
    private static final float COMPACT_HEIGHT_DP = 240;

    enum LayoutMode {
        FULL("full"),
        COMPACT("compact"),
        PHONE("phone"),
        NONE("none");

        final String value;

        LayoutMode(String value) {
            this.value = value;
        }

        static LayoutMode fromValue(String value) {
            for (LayoutMode mode : values()) {
                if (mode.value.equals(value)) {
                    return mode;
                }
            }
            return COMPACT;
        }
    }

    private static final Key[] KEYS = {
        new Key("=", KeyEvent.KEYCODE_LEFT_BRACKET),
        new Key("↑", KeyEvent.KEYCODE_DPAD_UP),
        new Key("=", KeyEvent.KEYCODE_RIGHT_BRACKET),
        new Key("←", KeyEvent.KEYCODE_DPAD_LEFT),
        new Key("OK", KeyEvent.KEYCODE_ENTER),
        new Key("→", KeyEvent.KEYCODE_DPAD_RIGHT),
        new Key("=", KeyEvent.KEYCODE_TAB),
        new Key("↓", KeyEvent.KEYCODE_DPAD_DOWN),
        new Key("=", KeyEvent.KEYCODE_ESCAPE),
        new Key("1", KeyEvent.KEYCODE_1),
        new Key("2", KeyEvent.KEYCODE_2),
        new Key("3", KeyEvent.KEYCODE_3),
        new Key("4", KeyEvent.KEYCODE_4),
        new Key("5", KeyEvent.KEYCODE_5),
        new Key("6", KeyEvent.KEYCODE_6),
        new Key("7", KeyEvent.KEYCODE_7),
        new Key("8", KeyEvent.KEYCODE_8),
        new Key("9", KeyEvent.KEYCODE_9),
        new Key("*", KeyEvent.KEYCODE_MINUS),
        new Key("0", KeyEvent.KEYCODE_0),
        new Key("#", KeyEvent.KEYCODE_EQUALS),
    };
    private static final int[] PHONE_KEY_INDICES = {
        0, 4, 2,
        9, 10, 11,
        12, 13, 14,
        15, 16, 17,
        18, 19, 20,
    };

    private final Paint buttonPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint textPaint = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final RectF[] keyBounds = new RectF[KEYS.length];
    private final int[] pressCounts = new int[KEYS.length];
    private final SparseIntArray pointerKeys = new SparseIntArray();
    private final float density;
    private LayoutMode layoutMode = LayoutMode.COMPACT;

    VirtualKeypadView(Context context) {
        this(context, null);
    }

    VirtualKeypadView(Context context, AttributeSet attrs) {
        super(context, attrs);
        density = getResources().getDisplayMetrics().density;
        setBackgroundColor(Color.rgb(16, 18, 20));
        setFocusable(false);
        setClickable(true);

        textPaint.setColor(Color.WHITE);
        textPaint.setTextAlign(Paint.Align.CENTER);
        textPaint.setTypeface(android.graphics.Typeface.DEFAULT_BOLD);

        for (int i = 0; i < keyBounds.length; i++) {
            keyBounds[i] = new RectF();
        }
    }

    @Override
    protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
        super.onLayout(changed, left, top, right, bottom);
        int width = right - left;
        int height = bottom - top;

        for (RectF bounds : keyBounds) {
            bounds.setEmpty();
        }
        if (width <= 0 || height <= 0) {
            return;
        }

        boolean compactLayout = layoutMode == LayoutMode.COMPACT;
        boolean phoneLayout = layoutMode == LayoutMode.PHONE;
        int columnCount = compactLayout ? COMPACT_COLUMN_COUNT : PHONE_COLUMN_COUNT;
        int rowCount = compactLayout ? 4 : phoneLayout ? PHONE_ROW_COUNT : FULL_ROW_COUNT;
        float gap = dp(KEY_GAP_DP);
        float groupGap = compactLayout ? dp(COMPACT_GROUP_GAP_DP) : gap;
        float horizontalPadding = dp(8);
        float verticalPadding = dp(VERTICAL_PADDING_DP);
        float availableKeyWidth =
            (
                width
                    - horizontalPadding * 2
                    - gap * (columnCount - 2)
                    - groupGap
            ) / columnCount;
        float keyHeight = (height - verticalPadding * 2 - gap * (rowCount - 1)) / rowCount;
        float keyWidth = availableKeyWidth;
        if (compactLayout) {
            keyWidth = Math.min(keyWidth, dp(COMPACT_MAX_KEY_WIDTH_DP));
            keyHeight = Math.min(keyHeight, dp(COMPACT_MAX_KEY_HEIGHT_DP));
        }

        float gridWidth =
            keyWidth * columnCount + gap * (columnCount - 2) + groupGap;
        float gridHeight = keyHeight * rowCount + gap * (rowCount - 1);
        float gridLeft = (width - gridWidth) / 2;
        float gridTop = (height - gridHeight) / 2;

        if (phoneLayout) {
            for (int slot = 0; slot < PHONE_KEY_INDICES.length; slot++) {
                int index = PHONE_KEY_INDICES[slot];
                int row = slot / PHONE_COLUMN_COUNT;
                int column = slot % PHONE_COLUMN_COUNT;
                float keyLeft = gridLeft + column * (keyWidth + gap);
                float keyTop = gridTop + row * (keyHeight + gap);
                keyBounds[index].set(
                    keyLeft,
                    keyTop,
                    keyLeft + keyWidth,
                    keyTop + keyHeight
                );
            }
        } else {
            for (int index = 0; index < KEYS.length; index++) {
                int row;
                int column;
                if (compactLayout && index >= 9) {
                    int numericIndex = index - 9;
                    row = numericIndex / PHONE_COLUMN_COUNT;
                    column = PHONE_COLUMN_COUNT + numericIndex % PHONE_COLUMN_COUNT;
                } else {
                    row = index / PHONE_COLUMN_COUNT;
                    column = index % PHONE_COLUMN_COUNT;
                }
                float keyLeft = gridLeft + column * (keyWidth + gap);
                if (compactLayout && column >= PHONE_COLUMN_COUNT) {
                    keyLeft += groupGap - gap;
                }
                float keyTop = gridTop + row * (keyHeight + gap);
                if (compactLayout && index < 9) {
                    keyTop += (keyHeight + gap) / 2;
                }
                keyBounds[index].set(
                    keyLeft,
                    keyTop,
                    keyLeft + keyWidth,
                    keyTop + keyHeight
                );
            }
        }

        textPaint.setTextSize(Math.min(dp(18), keyHeight * 0.32f));
    }

    void setLayoutMode(LayoutMode mode) {
        if (layoutMode == mode) {
            return;
        }
        releaseAllKeys();
        layoutMode = mode;
        requestLayout();
        invalidate();
    }

    int getPreferredHeight() {
        float height;
        if (layoutMode == LayoutMode.COMPACT) {
            height = COMPACT_HEIGHT_DP;
        } else if (layoutMode == LayoutMode.PHONE) {
            float fullKeyHeight =
                (
                    FULL_HEIGHT_DP
                        - VERTICAL_PADDING_DP * 2
                        - KEY_GAP_DP * (FULL_ROW_COUNT - 1)
                ) / FULL_ROW_COUNT;
            height =
                VERTICAL_PADDING_DP * 2
                    + fullKeyHeight * PHONE_ROW_COUNT
                    + KEY_GAP_DP * (PHONE_ROW_COUNT - 1);
        } else if (layoutMode == LayoutMode.FULL) {
            height = FULL_HEIGHT_DP;
        } else {
            height = 0;
        }
        return Math.round(dp(height));
    }

    @Override
    protected void onDraw(Canvas canvas) {
        super.onDraw(canvas);

        Paint.FontMetrics metrics = textPaint.getFontMetrics();
        for (int index = 0; index < KEYS.length; index++) {
            RectF bounds = keyBounds[index];
            if (bounds.isEmpty()) {
                continue;
            }
            boolean pressed = pressCounts[index] > 0;
            buttonPaint.setColor(pressed ? Color.rgb(55, 115, 205) : keyColor(index));
            canvas.drawRoundRect(bounds, dp(5), dp(5), buttonPaint);

            float baseline = bounds.centerY() - (metrics.ascent + metrics.descent) / 2;
            canvas.drawText(KEYS[index].label, bounds.centerX(), baseline, textPaint);
        }
    }

    @Override
    public boolean onTouchEvent(MotionEvent event) {
        int action = event.getActionMasked();
        int actionIndex = event.getActionIndex();

        switch (action) {
            case MotionEvent.ACTION_DOWN:
            case MotionEvent.ACTION_POINTER_DOWN:
                updatePointer(
                    event.getPointerId(actionIndex),
                    findKey(event.getX(actionIndex), event.getY(actionIndex))
                );
                break;
            case MotionEvent.ACTION_MOVE:
                for (int index = 0; index < event.getPointerCount(); index++) {
                    updatePointer(
                        event.getPointerId(index),
                        findKey(event.getX(index), event.getY(index))
                    );
                }
                break;
            case MotionEvent.ACTION_UP:
            case MotionEvent.ACTION_POINTER_UP:
                updatePointer(event.getPointerId(actionIndex), -1);
                performClick();
                break;
            case MotionEvent.ACTION_CANCEL:
                releaseAllKeys();
                break;
            default:
                break;
        }

        return true;
    }

    @Override
    public boolean performClick() {
        super.performClick();
        return true;
    }

    void releaseAllKeys() {
        for (int index = 0; index < KEYS.length; index++) {
            if (pressCounts[index] > 0) {
                pressCounts[index] = 0;
                SDLActivity.onNativeKeyUp(KEYS[index].androidKeyCode);
            }
        }
        pointerKeys.clear();
        invalidate();
    }

    @Override
    protected void onDetachedFromWindow() {
        releaseAllKeys();
        super.onDetachedFromWindow();
    }

    private void updatePointer(int pointerId, int newKeyIndex) {
        int mappingIndex = pointerKeys.indexOfKey(pointerId);
        int oldKeyIndex = mappingIndex >= 0 ? pointerKeys.valueAt(mappingIndex) : -1;
        if (oldKeyIndex == newKeyIndex) {
            return;
        }

        if (oldKeyIndex >= 0) {
            releaseKey(oldKeyIndex);
        }

        if (newKeyIndex >= 0) {
            pointerKeys.put(pointerId, newKeyIndex);
            pressKey(newKeyIndex);
        } else {
            pointerKeys.delete(pointerId);
        }
    }

    private void pressKey(int keyIndex) {
        if (pressCounts[keyIndex]++ == 0) {
            SDLActivity.onNativeKeyDown(KEYS[keyIndex].androidKeyCode);
            performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP);
        }
        invalidate();
    }

    private void releaseKey(int keyIndex) {
        if (pressCounts[keyIndex] > 0 && --pressCounts[keyIndex] == 0) {
            SDLActivity.onNativeKeyUp(KEYS[keyIndex].androidKeyCode);
        }
        invalidate();
    }

    private int findKey(float x, float y) {
        for (int index = 0; index < keyBounds.length; index++) {
            if (keyBounds[index].contains(x, y)) {
                return index;
            }
        }
        return -1;
    }

    private int keyColor(int index) {
        if (index < 9) {
            return Color.rgb(49, 54, 61);
        }
        return Color.rgb(39, 43, 48);
    }

    private float dp(float value) {
        return value * density;
    }

    private static final class Key {
        final String label;
        final int androidKeyCode;

        Key(String label, int androidKeyCode) {
            this.label = label;
            this.androidKeyCode = androidKeyCode;
        }
    }
}
