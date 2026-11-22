package com.laffy.unifiedstream.ui

import androidx.camera.view.PreviewView
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Cameraswitch
import androidx.compose.material.icons.rounded.Stop
import androidx.compose.material.icons.rounded.Videocam
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import com.laffy.unifiedstream.session.CameraStreamState
import com.laffy.unifiedstream.session.ConnectionState
import com.laffy.unifiedstream.ui.components.ConnectedChoiceGroup
import com.laffy.unifiedstream.ui.components.Notice
import com.laffy.unifiedstream.ui.components.SectionHeader
import com.laffy.unifiedstream.ui.theme.RecordingCoral
import com.laffy.unifiedstream.ui.theme.TechnicalFont
import com.laffy.unifiedstream.video.CameraFacing
import com.laffy.unifiedstream.video.CameraResolution

private val Viewfinder = Color(0xFF0A0D0D)
private val OverlayScrim = Color.Black.copy(alpha = .62f)

@Composable
fun CameraDestination(
    connection: ConnectionState,
    camera: CameraControls,
    modifier: Modifier = Modifier,
) {
    val state = camera.state
    val active = state is CameraStreamState.Active
    val problem = camera.permissionNeeded || state is CameraStreamState.Refused || state is CameraStreamState.Error
    val explanation = state.display ?: when {
        camera.permissionNeeded -> "Camera permission is required."
        !connection.isConnected -> "Connect to a PC before starting the camera."
        active -> "The live preview matches the transmitted source."
        state is CameraStreamState.Starting -> "Waiting for the PC to accept the stream…"
        else -> "Ready to stream MJPEG video."
    }

    DestinationList(modifier) {
        item { Viewfinder(camera, active) }
        item { Notice(explanation, error = problem) }
        item {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                SectionHeader("Resolution")
                ConnectedChoiceGroup(
                    options = CameraResolution.entries,
                    selected = camera.resolution,
                    label = { it.label },
                    onSelect = camera.onResolutionSelect,
                )
            }
        }
        item {
            FilledTonalButton(
                onClick = camera.onFacingToggle,
                enabled = connection.isConnected,
                shapes = ButtonDefaults.shapes(),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Icon(Icons.Rounded.Cameraswitch, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
                Text(
                    text = if (camera.facing == CameraFacing.BACK) "Use front camera" else "Use back camera",
                    modifier = Modifier.padding(start = 8.dp),
                )
            }
        }
        item { StartStopButton(active, connection.isConnected, camera.onToggle) }
    }
}

@Composable
private fun Viewfinder(camera: CameraControls, active: Boolean) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.extraLargeIncreased,
        color = Viewfinder,
    ) {
        Box(
            Modifier
                .fillMaxWidth()
                .aspectRatio(camera.resolution.width.toFloat() / camera.resolution.height)
                .background(Viewfinder),
        ) {
            val state = camera.state
            if (active && state is CameraStreamState.Active) {
                AndroidView(
                    factory = { context -> PreviewView(context).apply { implementationMode = PreviewView.ImplementationMode.COMPATIBLE } },
                    update = { camera.onPreviewSurface(it.surfaceProvider) },
                    modifier = Modifier.fillMaxSize().semantics { contentDescription = "Live camera preview" },
                )
                DisposableEffect(Unit) { onDispose { camera.onPreviewSurface(null) } }
                OverlayPill(Modifier.align(Alignment.TopStart).padding(14.dp)) {
                    Box(Modifier.size(8.dp).clip(CircleShape).background(RecordingCoral))
                    Text("REC", color = Color.White, style = MaterialTheme.typography.labelLarge)
                }
                OverlayPill(Modifier.align(Alignment.TopEnd).padding(14.dp)) {
                    Text(
                        text = "${state.params.width} × ${state.params.height}",
                        color = Color.White,
                        style = MaterialTheme.typography.labelLarge,
                        fontFamily = TechnicalFont,
                    )
                }
            } else {
                Column(
                    Modifier.align(Alignment.Center).padding(24.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Box(
                        Modifier.size(56.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary.copy(alpha = .16f)),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(Icons.Rounded.Videocam, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
                    }
                    Text("Preview appears while streaming", color = Color.White.copy(alpha = .7f), style = MaterialTheme.typography.bodyMedium)
                }
            }
        }
    }
}

@Composable
private fun OverlayPill(modifier: Modifier, content: @Composable () -> Unit) {
    Row(
        modifier = modifier.clip(CircleShape).background(OverlayScrim).padding(horizontal = 12.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) { content() }
}

@Composable
private fun StartStopButton(active: Boolean, connected: Boolean, onToggle: (Boolean) -> Unit) {
    val colors = if (active) {
        ButtonDefaults.buttonColors(
            containerColor = MaterialTheme.colorScheme.error,
            contentColor = MaterialTheme.colorScheme.onError,
        )
    } else {
        ButtonDefaults.buttonColors()
    }
    Button(
        onClick = { onToggle(!active) },
        enabled = connected,
        colors = colors,
        shapes = ButtonDefaults.shapes(),
        contentPadding = ButtonDefaults.contentPaddingFor(ButtonDefaults.LargeContainerHeight),
        modifier = Modifier.fillMaxWidth().heightIn(min = ButtonDefaults.LargeContainerHeight),
    ) {
        Icon(
            imageVector = if (active) Icons.Rounded.Stop else Icons.Rounded.Videocam,
            contentDescription = null,
            modifier = Modifier.size(ButtonDefaults.iconSizeFor(ButtonDefaults.LargeContainerHeight)),
        )
        Text(
            text = if (active) "Stop camera" else "Start camera",
            style = ButtonDefaults.textStyleFor(ButtonDefaults.LargeContainerHeight),
            modifier = Modifier.padding(start = 8.dp),
        )
    }
}
