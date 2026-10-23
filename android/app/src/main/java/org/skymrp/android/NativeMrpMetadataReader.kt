/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
package org.skymrp.android

import java.io.File

internal object NativeMrpMetadataReader : MrpMetadataReader {
    init {
        System.loadLibrary("SDL2")
        System.loadLibrary("skymrp")
    }

    override fun read(file: File): MrpMetadata? = nativeRead(file.absolutePath)

    private external fun nativeRead(path: String): MrpMetadata?
}
