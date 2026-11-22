package com.laffy.unifiedstream.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.DarkMode
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Stop
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedListItem
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.laffy.unifiedstream.discovery.DeviceIdentity
import com.laffy.unifiedstream.protocol.MAX_PAYLOAD
import com.laffy.unifiedstream.session.ConnectionState
import com.laffy.unifiedstream.transport.TestStreamReport
import com.laffy.unifiedstream.ui.components.MetricTile
import com.laffy.unifiedstream.ui.components.SectionHeader
import com.laffy.unifiedstream.ui.theme.TechnicalFont

@Composable
fun SettingsDestination(
    identity: DeviceIdentity?,
    connection: ConnectionState,
    darkTheme: Boolean,
    testReport: TestStreamReport?,
    testRunning: Boolean,
    onThemeChange: (Boolean) -> Unit,
    onStartTestStream: (Int, Int) -> Unit,
    onStopTestStream: () -> Unit,
    onDisconnect: () -> Unit,
    onCancelReconnect: () -> Unit,
    modifier: Modifier = Modifier,
) {
    DestinationList(modifier) {
        item { AppearanceItem(darkTheme, onThemeChange) }
        item { PhoneCard(identity, connection, onDisconnect, onCancelReconnect) }
        item { DiagnosticsCard(connection, testReport, testRunning, onStartTestStream, onStopTestStream) }
        item {
            Text(
                "UnifiedStream 0.1.0 · Trusted LAN MVP",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
            )
        }
    }
}

@Composable
private fun AppearanceItem(darkTheme: Boolean, onThemeChange: (Boolean) -> Unit) {
    SegmentedListItem(
        onClick = { onThemeChange(!darkTheme) },
        shapes = ListItemDefaults.segmentedShapes(index = 0, count = 1),
        colors = ListItemDefaults.segmentedColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
        leadingContent = { Icon(Icons.Rounded.DarkMode, contentDescription = null) },
        supportingContent = { Text("Uses the deep forest palette.") },
        trailingContent = {
            Switch(
                checked = darkTheme,
                onCheckedChange = null,
                thumbContent = if (darkTheme) {
                    { Icon(Icons.Rounded.Check, contentDescription = null, modifier = Modifier.size(SwitchIconSize)) }
                } else null,
            )
        },
        modifier = Modifier.semantics(mergeDescendants = true) {
            contentDescription = "Dark theme"
            role = Role.Switch
            stateDescription = if (darkTheme) "On" else "Off"
        },
    ) {
        Text("Dark theme", style = MaterialTheme.typography.titleMediumEmphasized)
    }
}

private val SwitchIconSize = 16.dp

@Composable
private fun PhoneCard(
    identity: DeviceIdentity?,
    connection: ConnectionState,
    onDisconnect: () -> Unit,
    onCancelReconnect: () -> Unit,
) {
    SettingsCard("This phone") {
        DetailRow("Name", identity?.name ?: "Loading…")
        DetailRow("Device ID", identity?.id ?: "—", technical = true)
        DetailRow("Session", connectionTitle(connection))
        if (connection is ConnectionState.Connected) DetailRow("Session ID", connection.sessionId.toULong().toString(), technical = true)
        when {
            connection is ConnectionState.Reconnecting -> OutlinedButton(onClick = onCancelReconnect, shapes = ButtonDefaults.shapes(), modifier = Modifier.fillMaxWidth()) { Text("Cancel reconnect") }
            connection.isConnected -> OutlinedButton(onClick = onDisconnect, shapes = ButtonDefaults.shapes(), modifier = Modifier.fillMaxWidth()) { Text("Disconnect") }
        }
    }
}

@Composable
private fun DiagnosticsCard(
    connection: ConnectionState,
    testReport: TestStreamReport?,
    testRunning: Boolean,
    onStart: (Int, Int) -> Unit,
    onStop: () -> Unit,
) {
    var rate by remember { mutableStateOf("60") }
    var frame by remember { mutableStateOf("4096") }
    val frameBytes = frame.toIntOrNull() ?: 0
    SettingsCard("Transport diagnostics", trailing = { RunningPill(testRunning) }) {
        Text(
            "A synthetic stream verifies fragmentation and integrity without changing media streams.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(value = rate, onValueChange = { rate = it }, label = { Text("Rate Hz") }, enabled = !testRunning, singleLine = true, shape = MaterialTheme.shapes.large, modifier = Modifier.weight(1f))
            OutlinedTextField(value = frame, onValueChange = { frame = it }, label = { Text("Frame bytes") }, enabled = !testRunning, singleLine = true, shape = MaterialTheme.shapes.large, modifier = Modifier.weight(1f))
        }
        if (frameBytes > MAX_PAYLOAD) {
            Text("This size exercises fragmentation.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Button(
            onClick = { if (testRunning) onStop() else onStart(rate.toIntOrNull() ?: 60, frameBytes.coerceAtLeast(8)) },
            enabled = connection.isConnected,
            shapes = ButtonDefaults.shapes(),
            modifier = Modifier.fillMaxWidth(),
        ) {
            Icon(if (testRunning) Icons.Rounded.Stop else Icons.Rounded.PlayArrow, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
            Text(if (testRunning) "Stop test" else "Start test", modifier = Modifier.padding(start = 8.dp))
        }
        testReport?.let {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                MetricTile("Verified", it.verified.toString(), Modifier.weight(1f))
                MetricTile("Missing", it.missing.toString(), Modifier.weight(1f))
                MetricTile("Corrupt", it.corrupt.toString(), Modifier.weight(1f))
            }
        }
    }
}

@Composable
private fun RunningPill(running: Boolean) {
    val colors = MaterialTheme.colorScheme
    Surface(
        shape = CircleShape,
        color = if (running) colors.primaryContainer else colors.surfaceContainerHighest,
        contentColor = if (running) colors.onPrimaryContainer else colors.onSurfaceVariant,
    ) {
        Row(
            Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Icon(if (running) Icons.Rounded.PlayArrow else Icons.Rounded.Stop, contentDescription = null, modifier = Modifier.size(16.dp))
            Text(if (running) "Running" else "Stopped", style = MaterialTheme.typography.labelLarge)
        }
    }
}

@Composable
private fun SettingsCard(
    title: String,
    trailing: @Composable (() -> Unit)? = null,
    content: @Composable () -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.extraLarge,
        color = MaterialTheme.colorScheme.surfaceContainer,
    ) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            SectionHeader(title, horizontalPadding = 0.dp, trailing = trailing)
            content()
        }
    }
}

@Composable
private fun DetailRow(label: String, value: String, technical: Boolean = false) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.Top) {
        Text(label, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.width(80.dp))
        Text(
            value,
            style = MaterialTheme.typography.bodyMedium,
            fontFamily = if (technical) TechnicalFont else null,
            modifier = Modifier.weight(1f),
            overflow = TextOverflow.Ellipsis,
        )
    }
}
