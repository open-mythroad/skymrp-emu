/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
package org.skymrp.android;

final class EmulatorSettings {
    static final String PREFERENCES = "skymrp_settings";
    static final String FULL_VM_ENABLED = "full_vm_enabled";
    static final String SCREEN_RESOLUTION = "screen_resolution";
    static final String KEYPAD_MODE = "keypad_mode";
    static final String STATUS_BAR_ENABLED = "status_bar_enabled";
    static final String AUTO_CHECK_UPDATES = "auto_check_updates";
    static final String LAST_UPDATE_CHECK = "last_update_check";

    static final String DEFAULT_SCREEN_RESOLUTION = "240x320";
    static final String DEFAULT_KEYPAD_MODE = "compact";
    static final boolean DEFAULT_STATUS_BAR_ENABLED = true;
    static final boolean DEFAULT_AUTO_CHECK_UPDATES = true;

    static final String[] SCREEN_RESOLUTIONS = {
        "128x160",
        "176x220",
        "240x320",
        "240x400",
        "320x240",
        "320x480",
        "480x800",
    };

    private EmulatorSettings() {
    }
}
