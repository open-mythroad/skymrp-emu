/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
package org.skymrp.android

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.AlertDialog
import androidx.compose.material.Card
import androidx.compose.material.MaterialTheme
import androidx.compose.material.Text
import androidx.compose.material.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.FirstBaseline
import androidx.compose.ui.layout.layout
import androidx.compose.ui.unit.dp

@Composable
internal fun UpdateAvailableDialog(
    release: GitHubRelease,
    onDismiss: () -> Unit,
    onViewRelease: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Update available") },
        text = {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .layout { measurable, constraints ->
                        val placeable = measurable.measure(constraints)
                        val compactHeight = (placeable.height - 12.dp.roundToPx()).coerceAtLeast(0)
                        layout(placeable.width, compactHeight) {
                            placeable.placeRelative(0, 0)
                        }
                    },
            ) {
                Text("A new version ${release.tagName} is available.")
                if (release.body.isNotBlank()) {
                    Spacer(modifier = Modifier.height(12.dp))
                    Card(
                        modifier = Modifier
                            .fillMaxWidth()
                            .heightIn(max = 300.dp)
                            .layout { measurable, constraints ->
                                val placeable = measurable.measure(constraints)
                                layout(
                                    placeable.width,
                                    placeable.height,
                                    alignmentLines = mapOf(FirstBaseline to placeable.height),
                                ) {
                                    placeable.placeRelative(0, 0)
                                }
                            },
                        backgroundColor = MaterialTheme.colors.onSurface.copy(alpha = 0.08f),
                        elevation = 0.dp,
                        shape = MaterialTheme.shapes.medium,
                    ) {
                        Column(
                            modifier = Modifier
                                .padding(12.dp)
                                .verticalScroll(rememberScrollState()),
                        ) {
                            Text(
                                text = release.body,
                                color = MaterialTheme.colors.onSurface.copy(alpha = 0.7f),
                                style = MaterialTheme.typography.body2,
                            )
                        }
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = onViewRelease) {
                Text("View release")
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) {
                Text("Later")
            }
        },
    )
}
