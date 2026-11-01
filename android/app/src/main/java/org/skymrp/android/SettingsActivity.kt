/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
package org.skymrp.android

import android.content.Context
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.AlertDialog
import androidx.compose.material.Checkbox
import androidx.compose.material.DropdownMenu
import androidx.compose.material.DropdownMenuItem
import androidx.compose.material.Icon
import androidx.compose.material.IconButton
import androidx.compose.material.MaterialTheme
import androidx.compose.material.Scaffold
import androidx.compose.material.Text
import androidx.compose.material.TextButton
import androidx.compose.material.TopAppBar
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.primarySurface
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

private data class ScreenResolution(
    val value: String,
) {
    val label = value.replace("x", " \u00d7 ")
}

private val screenResolutions =
    EmulatorSettings.SCREEN_RESOLUTIONS.map(::ScreenResolution)

private sealed interface UpdateCheckState {
    data object Idle : UpdateCheckState
    data object Checking : UpdateCheckState
    data class Available(val release: GitHubRelease) : UpdateCheckState
    data object UpToDate : UpdateCheckState
    data class Error(val message: String) : UpdateCheckState
}

class SettingsActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
        )
        super.onCreate(savedInstanceState)

        setContent {
            SkyMRPTheme {
                val context = LocalContext.current
                val uriHandler = LocalUriHandler.current
                val coroutineScope = rememberCoroutineScope()
                val preferences = remember(context) {
                    context.getSharedPreferences(
                        EmulatorSettings.PREFERENCES,
                        Context.MODE_PRIVATE,
                    )
                }
                val versionName = remember(context) {
                    context.packageManager
                        .getPackageInfo(context.packageName, 0)
                        .versionName
                        .orEmpty()
                }
                var fullVmEnabled by remember {
                    mutableStateOf(
                        preferences.getBoolean(EmulatorSettings.FULL_VM_ENABLED, false),
                    )
                }
                var screenResolution by remember {
                    mutableStateOf(
                        preferences.getString(
                            EmulatorSettings.SCREEN_RESOLUTION,
                            EmulatorSettings.DEFAULT_SCREEN_RESOLUTION,
                        ) ?: EmulatorSettings.DEFAULT_SCREEN_RESOLUTION,
                    )
                }
                var statusBarEnabled by remember {
                    mutableStateOf(
                        preferences.getBoolean(
                            EmulatorSettings.STATUS_BAR_ENABLED,
                            EmulatorSettings.DEFAULT_STATUS_BAR_ENABLED,
                        ),
                    )
                }
                var autoUpdatesEnabled by remember {
                    mutableStateOf(
                        preferences.getBoolean(
                            EmulatorSettings.AUTO_CHECK_UPDATES,
                            EmulatorSettings.DEFAULT_AUTO_CHECK_UPDATES,
                        ),
                    )
                }
                var resolutionMenuExpanded by remember { mutableStateOf(false) }
                var updateCheckState by remember {
                    mutableStateOf<UpdateCheckState>(UpdateCheckState.Idle)
                }
                val onFullVmEnabledChange: (Boolean) -> Unit = { enabled ->
                    fullVmEnabled = enabled
                    preferences.edit()
                        .putBoolean(EmulatorSettings.FULL_VM_ENABLED, enabled)
                        .apply()
                }
                val onStatusBarEnabledChange: (Boolean) -> Unit = { enabled ->
                    statusBarEnabled = enabled
                    preferences.edit()
                        .putBoolean(EmulatorSettings.STATUS_BAR_ENABLED, enabled)
                        .apply()
                }
                val onAutoUpdatesEnabledChange: (Boolean) -> Unit = { enabled ->
                    autoUpdatesEnabled = enabled
                    preferences.edit()
                        .putBoolean(EmulatorSettings.AUTO_CHECK_UPDATES, enabled)
                        .apply()
                }

                Scaffold(
                    modifier = Modifier.navigationBarsPadding(),
                    topBar = {
                        Column(
                            modifier = Modifier
                                .background(MaterialTheme.colors.primarySurface)
                                .statusBarsPadding(),
                        ) {
                            TopAppBar(
                                title = { Text("Settings") },
                                navigationIcon = {
                                    IconButton(onClick = ::finish) {
                                        Icon(
                                            imageVector =
                                                Icons.AutoMirrored.Outlined.ArrowBack,
                                            contentDescription = "Back",
                                        )
                                    }
                                },
                                elevation = 0.dp,
                            )
                        }
                    },
                ) { contentPadding ->
                    Column(
                        modifier = Modifier
                            .fillMaxSize()
                            .padding(contentPadding)
                            .verticalScroll(rememberScrollState()),
                    ) {
                        Text(
                            text = "Emulation",
                            modifier = Modifier.padding(
                                start = 16.dp,
                                top = 24.dp,
                                end = 16.dp,
                                bottom = 8.dp,
                            ),
                            color = MaterialTheme.colors.primary,
                            fontWeight = FontWeight.Medium,
                            style = MaterialTheme.typography.body2,
                        )
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 72.dp)
                                .toggleable(
                                    value = fullVmEnabled,
                                    role = Role.Checkbox,
                                    onValueChange = onFullVmEnabledChange,
                                )
                                .padding(horizontal = 16.dp, vertical = 12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Column(
                                modifier = Modifier.weight(1f),
                            ) {
                                Text(
                                    text = "Full VM",
                                    style = MaterialTheme.typography.body1,
                                )
                                Text(
                                    text = "Use the experimental Lua runtime (may be unstable)",
                                    modifier = Modifier.padding(top = 4.dp),
                                    color = MaterialTheme.colors.onBackground.copy(alpha = 0.6f),
                                    style = MaterialTheme.typography.body2,
                                )
                            }
                            Checkbox(
                                checked = fullVmEnabled,
                                onCheckedChange = null,
                            )
                        }
                        Box {
                            Row(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .heightIn(min = 72.dp)
                                    .clickable { resolutionMenuExpanded = true }
                                    .padding(horizontal = 16.dp, vertical = 12.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Column(
                                    modifier = Modifier.weight(1f),
                                ) {
                                    Text(
                                        text = "Screen resolution",
                                        style = MaterialTheme.typography.body1,
                                    )
                                    Text(
                                        text = screenResolutions
                                            .firstOrNull { it.value == screenResolution }
                                            ?.label
                                            ?: screenResolution,
                                        modifier = Modifier.padding(top = 4.dp),
                                        color = MaterialTheme.colors.onBackground.copy(
                                            alpha = 0.6f,
                                        ),
                                        style = MaterialTheme.typography.body2,
                                    )
                                }
                                Icon(
                                    imageVector = Icons.Filled.ArrowDropDown,
                                    contentDescription = "Select resolution",
                                    tint = MaterialTheme.colors.onBackground.copy(alpha = 0.6f),
                                )
                            }
                            Box(
                                modifier = Modifier
                                    .align(Alignment.TopEnd)
                                    .padding(end = 16.dp),
                            ) {
                                DropdownMenu(
                                    expanded = resolutionMenuExpanded,
                                    onDismissRequest = { resolutionMenuExpanded = false },
                                ) {
                                    screenResolutions.forEach { resolution ->
                                        DropdownMenuItem(
                                            onClick = {
                                                screenResolution = resolution.value
                                                preferences.edit()
                                                    .putString(
                                                        EmulatorSettings.SCREEN_RESOLUTION,
                                                        resolution.value,
                                                    )
                                                    .apply()
                                                resolutionMenuExpanded = false
                                            },
                                        ) {
                                            Text(
                                                text = resolution.label,
                                                color = if (
                                                    resolution.value == screenResolution
                                                ) {
                                                    MaterialTheme.colors.primary
                                                } else {
                                                    MaterialTheme.colors.onSurface
                                                },
                                            )
                                        }
                                    }
                                }
                            }
                        }
                        Text(
                            text = "Display",
                            modifier = Modifier.padding(
                                start = 16.dp,
                                top = 24.dp,
                                end = 16.dp,
                                bottom = 8.dp,
                            ),
                            color = MaterialTheme.colors.primary,
                            fontWeight = FontWeight.Medium,
                            style = MaterialTheme.typography.body2,
                        )
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 64.dp)
                                .toggleable(
                                    value = statusBarEnabled,
                                    role = Role.Checkbox,
                                    onValueChange = onStatusBarEnabledChange,
                                )
                                .padding(horizontal = 16.dp, vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                text = "Enable statusbar",
                                modifier = Modifier.weight(1f),
                                style = MaterialTheme.typography.body1,
                            )
                            Checkbox(
                                checked = statusBarEnabled,
                                onCheckedChange = null,
                            )
                        }
                        Text(
                            text = "Updates",
                            modifier = Modifier.padding(
                                start = 16.dp,
                                top = 24.dp,
                                end = 16.dp,
                                bottom = 8.dp,
                            ),
                            color = MaterialTheme.colors.primary,
                            fontWeight = FontWeight.Medium,
                            style = MaterialTheme.typography.body2,
                        )
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 72.dp)
                                .toggleable(
                                    value = autoUpdatesEnabled,
                                    role = Role.Checkbox,
                                    onValueChange = onAutoUpdatesEnabledChange,
                                )
                                .padding(horizontal = 16.dp, vertical = 12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    text = "Auto-check for updates",
                                    style = MaterialTheme.typography.body1,
                                )
                                Text(
                                    text = "Check for new releases on app startup",
                                    modifier = Modifier.padding(top = 4.dp),
                                    color = MaterialTheme.colors.onBackground.copy(alpha = 0.6f),
                                    style = MaterialTheme.typography.body2,
                                )
                            }
                            Checkbox(
                                checked = autoUpdatesEnabled,
                                onCheckedChange = null,
                            )
                        }
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = 64.dp)
                                .clickable(
                                    enabled = updateCheckState !is UpdateCheckState.Checking,
                                ) {
                                    updateCheckState = UpdateCheckState.Checking
                                    coroutineScope.launch {
                                        updateCheckState = fetchLatestRelease().fold(
                                            onSuccess = { release ->
                                                if (
                                                    isNewerVersion(
                                                        release.tagName,
                                                        versionName,
                                                    )
                                                ) {
                                                    UpdateCheckState.Available(release)
                                                } else {
                                                    UpdateCheckState.UpToDate
                                                }
                                            },
                                            onFailure = { error ->
                                                UpdateCheckState.Error(
                                                    error.localizedMessage
                                                        ?: "Could not check for updates",
                                                )
                                            },
                                        )
                                    }
                                }
                                .padding(horizontal = 16.dp, vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    text = "Check for updates",
                                    style = MaterialTheme.typography.body1,
                                )
                                Text(
                                    text = if (
                                        updateCheckState is UpdateCheckState.Checking
                                    ) {
                                        "Checking for updates..."
                                    } else {
                                        "Current version: $versionName"
                                    },
                                    modifier = Modifier.padding(top = 4.dp),
                                    color = MaterialTheme.colors.onBackground.copy(alpha = 0.6f),
                                    style = MaterialTheme.typography.body2,
                                )
                            }
                        }
                    }
                }

                when (val state = updateCheckState) {
                    is UpdateCheckState.Available -> {
                        UpdateAvailableDialog(
                            release = state.release,
                            onDismiss = {
                                updateCheckState = UpdateCheckState.Idle
                            },
                            onViewRelease = {
                                runCatching {
                                    uriHandler.openUri(state.release.htmlUrl)
                                }.onSuccess {
                                    updateCheckState = UpdateCheckState.Idle
                                }.onFailure { error ->
                                    updateCheckState = UpdateCheckState.Error(
                                        error.localizedMessage
                                            ?: "Could not open the release page",
                                    )
                                }
                            },
                        )
                    }

                    UpdateCheckState.UpToDate -> {
                        AlertDialog(
                            onDismissRequest = {
                                updateCheckState = UpdateCheckState.Idle
                            },
                            title = { Text("No updates available") },
                            text = {
                                Text("You have the latest version ($versionName).")
                            },
                            confirmButton = {
                                TextButton(
                                    onClick = {
                                        updateCheckState = UpdateCheckState.Idle
                                    },
                                ) {
                                    Text("OK")
                                }
                            },
                        )
                    }

                    is UpdateCheckState.Error -> {
                        AlertDialog(
                            onDismissRequest = {
                                updateCheckState = UpdateCheckState.Idle
                            },
                            title = { Text("Update check failed") },
                            text = { Text(state.message) },
                            confirmButton = {
                                TextButton(
                                    onClick = {
                                        updateCheckState = UpdateCheckState.Idle
                                    },
                                ) {
                                    Text("OK")
                                }
                            },
                        )
                    }

                    UpdateCheckState.Checking,
                    UpdateCheckState.Idle,
                    -> Unit
                }
            }
        }
    }
}
