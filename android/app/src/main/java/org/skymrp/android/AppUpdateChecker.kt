/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
package org.skymrp.android

import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

private const val LATEST_RELEASE_URL =
    "https://api.github.com/repos/open-mythroad/skymrp-emu/releases/latest"

internal const val UPDATE_CHECK_INTERVAL_MILLIS = 3_600_000L

internal data class GitHubRelease(
    val tagName: String,
    val body: String,
    val htmlUrl: String,
)

internal suspend fun fetchLatestRelease(): Result<GitHubRelease> =
    withContext(Dispatchers.IO) {
        runCatching {
            val connection = URL(LATEST_RELEASE_URL).openConnection() as HttpURLConnection
            try {
                connection.connectTimeout = 15_000
                connection.readTimeout = 15_000
                connection.setRequestProperty("Accept", "application/vnd.github+json")
                connection.setRequestProperty("User-Agent", "SkyMRP-Android")

                if (connection.responseCode != HttpURLConnection.HTTP_OK) {
                    throw IllegalStateException(
                        "Update server returned HTTP ${connection.responseCode}",
                    )
                }

                val json = connection.inputStream.bufferedReader().use { reader ->
                    JSONObject(reader.readText())
                }
                GitHubRelease(
                    tagName = json.getString("tag_name"),
                    body = json.optString("body"),
                    htmlUrl = json.getString("html_url"),
                )
            } finally {
                connection.disconnect()
            }
        }
    }

internal fun isNewerVersion(latest: String, current: String): Boolean {
    fun parts(version: String): List<Int> =
        version
            .trim()
            .removePrefix("v")
            .removePrefix("V")
            .split('.')
            .map { segment -> segment.takeWhile(Char::isDigit).toIntOrNull() ?: 0 }

    val latestParts = parts(latest)
    val currentParts = parts(current)
    repeat(maxOf(latestParts.size, currentParts.size)) { index ->
        val latestPart = latestParts.getOrElse(index) { 0 }
        val currentPart = currentParts.getOrElse(index) { 0 }
        if (latestPart > currentPart) return true
        if (latestPart < currentPart) return false
    }
    return false
}
