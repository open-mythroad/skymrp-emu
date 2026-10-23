/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
package org.skymrp.android

import java.io.File
import java.io.IOException
import java.util.ArrayDeque

internal data class MrpListItem(
    val name: String,
    val path: String,
    val vendor: String,
    val description: String = "",
    val version: Long,
    val isDirectory: Boolean = false,
    val parentPath: String = "",
)

internal enum class MrpScanMode {
    FOLDER,
    PARSED,
}

internal data class MrpMetadata(
    val name: String = "",
    val vendor: String = "",
    val description: String = "",
    val version: Long = 0,
)

internal fun interface MrpMetadataReader {
    fun read(file: File): MrpMetadata?
}

internal class MrpFileScanner(
    private val rootDirectory: File,
    private val metadataReader: MrpMetadataReader? = null,
) {
    private val itemComparator =
        compareBy<MrpListItem> { !it.isDirectory }
            .thenBy(String.CASE_INSENSITIVE_ORDER) { it.name }
            .thenBy(String.CASE_INSENSITIVE_ORDER) { it.path }

    fun scan(
        directory: File,
        query: String,
        mode: MrpScanMode,
    ): List<MrpListItem> {
        return when (mode) {
            MrpScanMode.FOLDER -> {
                if (query.isBlank()) {
                    list(directory)
                } else {
                    search(directory, query)
                }
            }
            MrpScanMode.PARSED -> listParsed(directory, query)
        }
    }

    fun list(directory: File): List<MrpListItem> {
        val canonicalDirectory = prepareDirectory(directory) ?: return emptyList()

        return canonicalDirectory.listFiles()
            ?.asSequence()
            ?.filter(::isVisibleEntry)
            ?.map { file -> file.toListItem(canonicalDirectory) }
            ?.sortedWith(itemComparator)
            ?.toList()
            .orEmpty()
    }

    fun search(directory: File, query: String): List<MrpListItem> {
        val normalizedQuery = query.trim()
        if (normalizedQuery.isEmpty()) {
            return list(directory)
        }

        val canonicalDirectory = prepareDirectory(directory) ?: return emptyList()

        val results = mutableListOf<MrpListItem>()
        val pendingDirectories = ArrayDeque<File>()
        val visitedDirectories = mutableSetOf<String>()
        pendingDirectories.add(canonicalDirectory)

        while (pendingDirectories.isNotEmpty()) {
            val currentDirectory = pendingDirectories.removeFirst()
            if (!visitedDirectories.add(currentDirectory.path)) {
                continue
            }

            currentDirectory.listFiles()?.forEach { file ->
                val canonicalFile = canonicalFileOrNull(file) ?: return@forEach
                if (!isWithinRoot(canonicalFile)) {
                    return@forEach
                }

                if (canonicalFile.isDirectory) {
                    pendingDirectories.add(canonicalFile)
                    if (canonicalFile.name.contains(normalizedQuery, ignoreCase = true)) {
                        results.add(canonicalFile.toListItem(canonicalDirectory))
                    }
                } else if (
                    canonicalFile.isFile &&
                    canonicalFile.extension.equals("mrp", ignoreCase = true)
                ) {
                    val item = canonicalFile.toListItem(canonicalDirectory)
                    if (
                        item.name.contains(normalizedQuery, ignoreCase = true) ||
                        item.vendor.contains(normalizedQuery, ignoreCase = true)
                    ) {
                        results.add(item)
                    }
                }
            }
        }

        return results.sortedWith(itemComparator)
    }

    private fun listParsed(directory: File, query: String): List<MrpListItem> {
        val canonicalDirectory = prepareDirectory(directory) ?: return emptyList()
        val normalizedQuery = query.trim()
        val results = mutableListOf<MrpListItem>()
        val pendingDirectories = ArrayDeque<File>()
        val visitedDirectories = mutableSetOf<String>()
        pendingDirectories.add(canonicalDirectory)

        while (pendingDirectories.isNotEmpty()) {
            val currentDirectory = pendingDirectories.removeFirst()
            if (!visitedDirectories.add(currentDirectory.path)) {
                continue
            }

            currentDirectory.listFiles()?.forEach { file ->
                val canonicalFile = canonicalFileOrNull(file) ?: return@forEach
                if (!isWithinRoot(canonicalFile)) {
                    return@forEach
                }

                if (canonicalFile.isDirectory) {
                    pendingDirectories.add(canonicalFile)
                } else if (
                    canonicalFile.isFile &&
                    canonicalFile.extension.equals("mrp", ignoreCase = true)
                ) {
                    val item = canonicalFile.toListItem(
                        searchRoot = canonicalDirectory,
                        readMetadata = true,
                    )
                    if (
                        normalizedQuery.isEmpty() ||
                        item.name.contains(normalizedQuery, ignoreCase = true) ||
                        item.vendor.contains(normalizedQuery, ignoreCase = true)
                    ) {
                        results.add(item)
                    }
                }
            }
        }

        return results.sortedWith(itemComparator)
    }

    private fun isVisibleEntry(file: File): Boolean {
        val canonicalFile = canonicalFileOrNull(file) ?: return false
        return isWithinRoot(canonicalFile) &&
            (
                canonicalFile.isDirectory ||
                    (
                        canonicalFile.isFile &&
                            canonicalFile.extension.equals("mrp", ignoreCase = true)
                        )
                )
    }

    private fun prepareDirectory(directory: File): File? {
        val canonicalDirectory = canonicalFileOrNull(directory) ?: return null
        val canonicalRoot = canonicalFileOrNull(rootDirectory) ?: return null
        if (canonicalDirectory == canonicalRoot && !canonicalDirectory.exists()) {
            if (!canonicalDirectory.mkdirs()) {
                return null
            }
        }
        if (!isWithinRoot(canonicalDirectory) || !canonicalDirectory.isDirectory) {
            return null
        }
        return canonicalDirectory
    }

    private fun File.toListItem(
        searchRoot: File,
        readMetadata: Boolean = false,
    ): MrpListItem {
        val parent = parentFile
        val relativeParent = if (parent == null || parent == searchRoot) {
            ""
        } else {
            parent.relativeTo(searchRoot).invariantSeparatorsPath
        }
        val metadata = if (readMetadata) {
            try {
                metadataReader?.read(this)
            } catch (_: Exception) {
                null
            }
        } else {
            null
        }
        return MrpListItem(
            name = metadata?.name?.takeIf(String::isNotBlank) ?: name,
            path = absolutePath,
            vendor = metadata?.vendor.orEmpty(),
            description = metadata?.description.orEmpty(),
            version = metadata?.version ?: 0,
            isDirectory = isDirectory,
            parentPath = relativeParent,
        )
    }

    private fun isWithinRoot(file: File): Boolean {
        val canonicalRoot = canonicalFileOrNull(rootDirectory) ?: return false
        return file == canonicalRoot || file.path.startsWith(canonicalRoot.path + File.separator)
    }

    private fun canonicalFileOrNull(file: File): File? {
        return try {
            file.canonicalFile
        } catch (_: IOException) {
            null
        } catch (_: SecurityException) {
            null
        }
    }
}
