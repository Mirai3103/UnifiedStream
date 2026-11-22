package com.laffy.unifiedstream.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.background
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowForward
import androidx.compose.material.icons.automirrored.rounded.VolumeOff
import androidx.compose.material.icons.automirrored.rounded.VolumeUp
import androidx.compose.material.icons.rounded.Computer
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.Mic
import androidx.compose.material.icons.rounded.MicOff
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.Videocam
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ContainedLoadingIndicator
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedToggleButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.laffy.unifiedstream.session.CameraStreamState
import com.laffy.unifiedstream.session.ConnectionState
import com.laffy.unifiedstream.session.MicStreamState
import com.laffy.unifiedstream.session.SpeakerStreamState
import com.laffy.unifiedstream.ui.components.LevelMeter
import com.laffy.unifiedstream.ui.components.MetricTile
import com.laffy.unifiedstream.ui.components.QualityPill
import com.laffy.unifiedstream.ui.components.SectionHeader
import com.laffy.unifiedstream.ui.components.StreamCard
import java.util.Locale

@Composable
fun HomeDestination(
    connection: ConnectionState,
    dashboard: DashboardState,
    mic: MicControls,
    camera: CameraControls,
    speaker: SpeakerControls,
    onOpenDevices: () -> Unit,
    onOpenCamera: () -> Unit,
    modifier: Modifier = Modifier,
) {
    DestinationList(modifier) {
        item { ConnectionHero(connection, onOpenDevices) }
        item { NetworkSection(dashboard) }
        item { SectionHeader("Streams", Modifier.padding(top = 8.dp)) }
        item { CameraCard(connection.isConnected, camera, onOpenCamera) }
        item { MicrophoneCard(connection.isConnected, mic) }
        item { SpeakerCard(connection.isConnected, speaker) }
    }
}

@Composable
private fun ConnectionHero(connection: ConnectionState, onFindComputer: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    val container = when {
        connection.isConnected -> colors.primaryContainer
        connection is ConnectionState.Failed -> colors.errorContainer
        connection.isBusy -> colors.secondaryContainer
        else -> colors.surfaceContainerHigh
    }
    val content = when {
        connection.isConnected -> colors.onPrimaryContainer
        connection is ConnectionState.Failed -> colors.onErrorContainer
        connection.isBusy -> colors.onSecondaryContainer
        else -> colors.onSurface
    }
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.extraLargeIncreased,
        color = container,
        contentColor = content,
    ) {
        Column(Modifier.padding(24.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            HeroBadge(connection)
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(
                    text = if (connection.isConnected) "Connected" else if (connection is ConnectionState.Failed) "Failed" else if (connection.isBusy) "Working" else "Offline",
                    style = MaterialTheme.typography.labelLarge,
                )
                Text(connectionTitle(connection), style = MaterialTheme.typography.headlineMediumEmphasized)
                Text(connectionDetail(connection), style = MaterialTheme.typography.bodyMedium)
            }
            if (!connection.isConnected && !connection.isBusy) {
                Button(
                    onClick = onFindComputer,
                    colors = if (connection is ConnectionState.Failed) {
                        ButtonDefaults.buttonColors(containerColor = content, contentColor = container)
                    } else {
                        ButtonDefaults.buttonColors()
                    },
                    shapes = ButtonDefaults.shapes(),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Icon(Icons.Rounded.Search, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
                    Text("Find a computer", modifier = Modifier.padding(start = 8.dp))
                }
            }
        }
    }
}

/** The connection state as a shape: cookie when connected, loader while working, burst on failure. */
@Composable
private fun HeroBadge(connection: ConnectionState) {
    val colors = MaterialTheme.colorScheme
    when {
        connection.isBusy -> ContainedLoadingIndicator(Modifier.size(72.dp))
        else -> {
            val (shape, tint, icon) = when {
                connection.isConnected -> Triple(MaterialShapes.Cookie9Sided.toShape(), colors.primary, Icons.Rounded.Computer)
                connection is ConnectionState.Failed -> Triple(MaterialShapes.SoftBurst.toShape(), colors.error, Icons.Rounded.ErrorOutline)
                else -> Triple(MaterialShapes.Clover4Leaf.toShape(), colors.surfaceContainerHighest, Icons.Rounded.Computer)
            }
            val onTint = when {
                connection.isConnected -> colors.onPrimary
                connection is ConnectionState.Failed -> colors.onError
                else -> colors.onSurfaceVariant
            }
            Box(Modifier.size(72.dp).clip(shape).background(tint), contentAlignment = Alignment.Center) {
                Icon(icon, contentDescription = null, tint = onTint, modifier = Modifier.size(36.dp))
            }
        }
    }
}

@Composable
private fun NetworkSection(dashboard: DashboardState) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        SectionHeader("Network", trailing = { QualityPill(dashboard.quality) })
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MetricTile("Latency", dashboard.rttMs?.let { String.format(Locale.US, "%.1f ms", it) } ?: "—", Modifier.weight(1f), MaterialTheme.shapes.extraLarge)
            MetricTile("Upload", String.format(Locale.US, "%.2f Mbps", dashboard.txMbps), Modifier.weight(1f), MaterialTheme.shapes.large)
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MetricTile("Loss", String.format(Locale.US, "%.2f %%", dashboard.lossPct), Modifier.weight(1f), MaterialTheme.shapes.large)
            MetricTile("Jitter", String.format(Locale.US, "%.2f ms", dashboard.jitterMs), Modifier.weight(1f), MaterialTheme.shapes.extraLarge)
        }
    }
}

@Composable
private fun CameraCard(connected: Boolean, camera: CameraControls, onOpenCamera: () -> Unit) {
    val state = camera.state
    val pending = state is CameraStreamState.Starting
    val active = state is CameraStreamState.Active
    val problem = state is CameraStreamState.Refused || state is CameraStreamState.Error || camera.permissionNeeded
    val status = state.display ?: when {
        camera.permissionNeeded -> "Camera permission is required."
        active -> "Camera is streaming to the PC."
        pending -> "Waiting for the PC to accept the stream…"
        !connected -> "Connect to a PC to use the camera."
        else -> "Use this phone as the PC camera."
    }
    StreamCard(
        title = "Camera",
        status = status,
        icon = Icons.Rounded.Videocam,
        checked = active || pending,
        pending = pending,
        toggleDescription = "Camera stream",
        toggleEnabled = connected,
        onCheckedChange = camera.onToggle,
        statusIsError = problem,
    ) {
        FilledTonalButton(onClick = onOpenCamera, shapes = ButtonDefaults.shapes(), modifier = Modifier.fillMaxWidth()) {
            Text("Open camera controls")
            Icon(Icons.AutoMirrored.Rounded.ArrowForward, contentDescription = null, modifier = Modifier.padding(start = 8.dp).size(ButtonDefaults.IconSize))
        }
    }
}

@Composable
private fun MicrophoneCard(connected: Boolean, mic: MicControls) {
    val state = mic.state
    val pending = state is MicStreamState.Starting
    val active = state is MicStreamState.Active
    val problem = state is MicStreamState.Refused || state is MicStreamState.Error || mic.permissionNeeded
    val status = when {
        active && mic.muted -> "Muted"
        active -> "Streaming · ${(state as MicStreamState.Active).params.sampleRate / 1000} kHz"
        else -> state.display ?: when {
            mic.permissionNeeded -> "Microphone permission is required."
            !connected -> "Connect to a PC to use the microphone."
            else -> "Use this phone as the PC microphone."
        }
    }
    StreamCard(
        title = "Microphone",
        status = status,
        icon = if (active && mic.muted) Icons.Rounded.MicOff else Icons.Rounded.Mic,
        checked = active || pending,
        pending = pending,
        toggleDescription = "Microphone stream",
        toggleEnabled = connected,
        onCheckedChange = mic.onToggle,
        statusIsError = problem,
    ) {
        if (active) {
            LevelMeter(mic.level.rms, "Microphone level", live = !mic.muted)
            MuteToggle(muted = mic.muted, onMuteToggle = mic.onMuteToggle, mutedIcon = Icons.Rounded.MicOff, liveIcon = Icons.Rounded.Mic)
            Column(verticalArrangement = Arrangement.spacedBy(0.dp)) {
                Text("Gain ${String.format(Locale.US, "%.1f×", mic.gain)}", style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                ValueSlider(mic.gain, mic.onGainChange, 0.5f..4f, "Microphone gain")
            }
            if (mic.noiseSuppressionAvailable) {
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text("Noise suppression", style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                    Switch(
                        checked = mic.noiseSuppression,
                        onCheckedChange = mic.onNoiseSuppressionToggle,
                        modifier = Modifier.semantics { contentDescription = "Noise suppression" },
                    )
                }
            }
        }
    }
}

@Composable
private fun SpeakerCard(connected: Boolean, speaker: SpeakerControls) {
    val state = speaker.state
    val pending = state is SpeakerStreamState.Requesting
    val active = state is SpeakerStreamState.Active
    val problem = state is SpeakerStreamState.Refused || state is SpeakerStreamState.Error
    val status = when {
        active && speaker.muted -> "Muted"
        active -> "Playing PC audio"
        else -> state.display ?: if (!connected) "Connect to a PC to play its audio here." else "Play PC audio through this phone."
    }
    StreamCard(
        title = "Speaker",
        status = status,
        icon = if (active && speaker.muted) Icons.AutoMirrored.Rounded.VolumeOff else Icons.AutoMirrored.Rounded.VolumeUp,
        checked = active || pending,
        pending = pending,
        toggleDescription = "Speaker stream",
        toggleEnabled = connected,
        onCheckedChange = speaker.onToggle,
        statusIsError = problem,
    ) {
        if (active) {
            LevelMeter(speaker.level.rms, "Speaker level", live = !speaker.muted)
            MuteToggle(muted = speaker.muted, onMuteToggle = speaker.onMuteToggle, mutedIcon = Icons.AutoMirrored.Rounded.VolumeOff, liveIcon = Icons.AutoMirrored.Rounded.VolumeUp)
            Column {
                Text("Volume ${(speaker.volume * 100).toInt()}%", style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                ValueSlider(speaker.volume, speaker.onVolumeChange, 0f..1f, "Speaker volume")
            }
        }
    }
}

@Composable
private fun MuteToggle(
    muted: Boolean,
    onMuteToggle: (Boolean) -> Unit,
    mutedIcon: androidx.compose.ui.graphics.vector.ImageVector,
    liveIcon: androidx.compose.ui.graphics.vector.ImageVector,
) {
    OutlinedToggleButton(
        checked = muted,
        onCheckedChange = onMuteToggle,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Icon(if (muted) mutedIcon else liveIcon, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
        Text(if (muted) "Unmute" else "Mute", modifier = Modifier.padding(start = 8.dp))
    }
}
