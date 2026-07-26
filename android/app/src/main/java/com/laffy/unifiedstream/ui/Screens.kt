package com.laffy.unifiedstream.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.laffy.unifiedstream.discovery.DiscoveredDevice
import com.laffy.unifiedstream.discovery.DiscoveryState
import com.laffy.unifiedstream.discovery.ManualAddress
import com.laffy.unifiedstream.protocol.MAX_PAYLOAD
import com.laffy.unifiedstream.session.ConnectionState
import com.laffy.unifiedstream.telemetry.LinkQuality
import com.laffy.unifiedstream.transport.TestStreamReport
import com.laffy.unifiedstream.ui.theme.Degraded
import com.laffy.unifiedstream.ui.theme.Good
import com.laffy.unifiedstream.ui.theme.Poor
import com.laffy.unifiedstream.ui.theme.Surface1
import com.laffy.unifiedstream.ui.theme.Surface2
import com.laffy.unifiedstream.ui.theme.TextMuted

/** Features that exist in the UI but have no implementation behind them yet. */
private val PLANNED_FEATURES = listOf(
    Triple("cam", "Camera", "Stream this camera to the PC"),
    Triple("mic", "Microphone", "Use this mic as a PC input"),
    Triple("spk", "Speaker", "Play PC audio through this phone"),
)

@Composable
private fun SectionCard(
    title: String,
    modifier: Modifier = Modifier,
    trailing: @Composable (() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    Card(
        modifier = modifier.fillMaxWidth(),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(containerColor = Surface1),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = title.uppercase(),
                    style = MaterialTheme.typography.labelMedium,
                    color = TextMuted,
                )
                trailing?.invoke()
            }
            content()
        }
    }
}

// --- Device list --------------------------------------------------------------------------

@Composable
fun DeviceListScreen(
    devices: List<DiscoveredDevice>,
    discoveryState: DiscoveryState,
    offerManual: Boolean,
    manualError: ManualAddress.Error?,
    onSelect: (DiscoveredDevice) -> Unit,
    onAddManual: (String, String) -> Unit,
    onManualEdited: () -> Unit,
    modifier: Modifier = Modifier,
) {
    LazyColumn(
        modifier = modifier
            .fillMaxSize()
            .padding(horizontal = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            Text(
                text = "Devices",
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.padding(top = 8.dp),
            )
        }

        item { DiscoveryStatusRow(discoveryState, devices.isEmpty()) }

        items(devices, key = { it.id }) { device ->
            DeviceRow(device = device, onClick = { onSelect(device) })
        }

        if (offerManual) {
            item {
                ManualEntryCard(
                    error = manualError,
                    onSubmit = onAddManual,
                    onEdited = onManualEdited,
                )
            }
        }

        item { Spacer(Modifier.height(24.dp)) }
    }
}

@Composable
private fun DiscoveryStatusRow(state: DiscoveryState, empty: Boolean) {
    val message = when (state) {
        DiscoveryState.Idle -> "Not searching"
        DiscoveryState.NoNetwork ->
            "Not connected to Wi-Fi. Join the same network as your PC."
        DiscoveryState.Searching -> "Searching for PCs on this network…"
        DiscoveryState.Found -> if (empty) "Searching…" else "Tap a device to connect"
        DiscoveryState.TimedOut ->
            "No PCs found. Some networks block discovery — enter the address by hand."
        is DiscoveryState.Failed -> state.message
    }

    Row(verticalAlignment = Alignment.CenterVertically) {
        if (state is DiscoveryState.Searching) {
            CircularProgressIndicator(
                modifier = Modifier.size(14.dp),
                strokeWidth = 2.dp,
                color = MaterialTheme.colorScheme.primary,
            )
            Spacer(Modifier.width(10.dp))
        }
        Text(
            text = message,
            style = MaterialTheme.typography.bodyMedium,
            color = if (state is DiscoveryState.Failed) Poor else TextMuted,
        )
    }
}

@Composable
private fun DeviceRow(device: DiscoveredDevice, onClick: () -> Unit) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(enabled = device.isCompatible, onClick = onClick),
        shape = RoundedCornerShape(16.dp),
        colors = CardDefaults.cardColors(containerColor = Surface1),
    ) {
        Row(
            Modifier
                .fillMaxWidth()
                .padding(16.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(
                    text = device.name,
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    text = "${device.host}:${device.port}",
                    style = MaterialTheme.typography.bodySmall,
                    color = TextMuted,
                    fontFamily = FontFamily.Monospace,
                )
                if (device.caps.isNotEmpty()) {
                    Text(
                        text = device.caps.joinToString(" · "),
                        style = MaterialTheme.typography.bodySmall,
                        color = TextMuted,
                    )
                }
                if (!device.isCompatible) {
                    Text(
                        text = "Incompatible version (v${device.protocolVersion})",
                        style = MaterialTheme.typography.bodySmall,
                        color = Poor,
                    )
                }
            }
            if (device.manual) {
                Text("manual", style = MaterialTheme.typography.labelSmall, color = TextMuted)
            }
        }
    }
}

@Composable
private fun ManualEntryCard(
    error: ManualAddress.Error?,
    onSubmit: (String, String) -> Unit,
    onEdited: () -> Unit,
) {
    var host by remember { mutableStateOf("") }
    var port by remember { mutableStateOf("47810") }

    SectionCard(title = "Connect by address") {
        OutlinedTextField(
            value = host,
            onValueChange = {
                host = it
                onEdited()
            },
            label = { Text("IP address or hostname") },
            singleLine = true,
            isError = error == ManualAddress.Error.EMPTY_HOST ||
                error == ManualAddress.Error.MALFORMED_HOST,
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = port,
            onValueChange = {
                port = it
                onEdited()
            },
            label = { Text("Port") },
            singleLine = true,
            isError = error == ManualAddress.Error.MALFORMED_PORT ||
                error == ManualAddress.Error.PORT_OUT_OF_RANGE,
            modifier = Modifier.fillMaxWidth(),
        )
        error?.let {
            Text(
                text = when (it) {
                    ManualAddress.Error.EMPTY_HOST -> "Enter an address"
                    ManualAddress.Error.MALFORMED_HOST -> "That is not a valid address"
                    ManualAddress.Error.MALFORMED_PORT -> "Port must be a number"
                    ManualAddress.Error.PORT_OUT_OF_RANGE -> "Port must be between 1 and 65535"
                },
                style = MaterialTheme.typography.bodySmall,
                color = Poor,
            )
        }
        Button(onClick = { onSubmit(host, port) }, modifier = Modifier.fillMaxWidth()) {
            Text("Connect")
        }
    }
}

// --- Session ------------------------------------------------------------------------------

@Composable
fun SessionScreen(
    connection: ConnectionState,
    dashboard: DashboardState,
    testReport: TestStreamReport?,
    testRunning: Boolean,
    onStartTestStream: (Int, Int) -> Unit,
    onStopTestStream: () -> Unit,
    onDisconnect: () -> Unit,
    onCancelReconnect: () -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    // Scrollable: the cards plus the test-stream metrics overflow a phone screen, and without
    // this the disconnect control at the bottom is simply unreachable.
    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        ConnectionHeader(connection)

        SectionCard(
            title = "Link quality",
            trailing = { QualityBadge(dashboard.quality) },
        ) {
            if (connection.isConnected) {
                MetricGrid(dashboard)
            } else {
                Text(
                    text = "Metrics appear once a session is connected.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = TextMuted,
                )
            }
        }

        TestStreamCard(
            connected = connection.isConnected,
            report = testReport,
            running = testRunning,
            onStart = onStartTestStream,
            onStop = onStopTestStream,
        )

        SectionCard(title = "Streams") {
            PLANNED_FEATURES.forEach { (id, label, hint) ->
                FeatureToggleRow(key = id, label = label, hint = hint)
            }
            Text(
                text = "Camera, microphone, and speaker arrive in later changes.",
                style = MaterialTheme.typography.bodySmall,
                color = TextMuted,
            )
        }

        when {
            connection is ConnectionState.Reconnecting ->
                OutlinedButton(onClick = onCancelReconnect, modifier = Modifier.fillMaxWidth()) {
                    Text("Cancel")
                }

            connection.isConnected ->
                OutlinedButton(onClick = onDisconnect, modifier = Modifier.fillMaxWidth()) {
                    Text("Disconnect")
                }

            else ->
                TextButton(onClick = onBack, modifier = Modifier.fillMaxWidth()) {
                    Text("Back to devices")
                }
        }

        Spacer(Modifier.height(24.dp))
    }
}

@Composable
private fun TestStreamCard(
    connected: Boolean,
    report: TestStreamReport?,
    running: Boolean,
    onStart: (Int, Int) -> Unit,
    onStop: () -> Unit,
) {
    var rate by remember { mutableStateOf("60") }
    var frame by remember { mutableStateOf("4096") }
    val frameBytes = frame.toIntOrNull() ?: 0

    SectionCard(title = "Test stream") {
        Text(
            text = "No codecs yet — this proves the transport.",
            style = MaterialTheme.typography.bodySmall,
            color = TextMuted,
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(
                value = rate,
                onValueChange = { rate = it },
                label = { Text("Rate (Hz)") },
                singleLine = true,
                enabled = !running,
                modifier = Modifier.weight(1f),
            )
            OutlinedTextField(
                value = frame,
                onValueChange = { frame = it },
                label = { Text("Frame (bytes)") },
                singleLine = true,
                enabled = !running,
                modifier = Modifier.weight(1f),
            )
        }
        if (frameBytes > MAX_PAYLOAD) {
            Text(
                text = "Above $MAX_PAYLOAD bytes, so this exercises fragmentation.",
                style = MaterialTheme.typography.bodySmall,
                color = TextMuted,
            )
        }
        Button(
            onClick = {
                if (running) {
                    onStop()
                } else {
                    onStart(rate.toIntOrNull() ?: 60, frameBytes.coerceAtLeast(8))
                }
            },
            enabled = connected,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Text(if (running) "Stop" else "Start")
        }
        report?.let {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Metric("Verified", it.verified.toString(), Modifier.weight(1f))
                Metric("Missing", it.missing.toString(), Modifier.weight(1f))
                Metric("Corrupt", it.corrupt.toString(), Modifier.weight(1f))
            }
        }
    }
}

@Composable
private fun ConnectionHeader(connection: ConnectionState) {
    val (label, color) = when (connection) {
        is ConnectionState.Connected -> connection.peerName to Good
        is ConnectionState.Connecting -> "Connecting to ${connection.peerName}" to Degraded
        is ConnectionState.Reconnecting ->
            "Reconnecting (${connection.attempt}/${connection.maxAttempts})" to Degraded
        is ConnectionState.Failed -> connection.reason.display to Poor
        ConnectionState.Discovering -> "Searching" to TextMuted
        ConnectionState.Idle -> "Not connected" to TextMuted
    }

    Column {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier
                    .size(10.dp)
                    .clip(CircleShape)
                    .background(color),
            )
            Spacer(Modifier.width(10.dp))
            Text(
                text = label,
                style = MaterialTheme.typography.headlineSmall,
                fontWeight = FontWeight.SemiBold,
            )
        }
        (connection as? ConnectionState.Connected)?.let { state ->
            Text(
                text = "session ${state.sessionId.toULong().toString(16)}",
                style = MaterialTheme.typography.bodySmall,
                color = TextMuted,
                fontFamily = FontFamily.Monospace,
            )
        }
    }
}

@Composable
private fun MetricGrid(dashboard: DashboardState) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Metric("Ping", dashboard.rttMs?.let { "%.1f ms".format(it) } ?: "—", Modifier.weight(1f))
            Metric("Jitter", "%.2f ms".format(dashboard.jitterMs), Modifier.weight(1f))
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Metric("Sent", "%.2f Mbps".format(dashboard.txMbps), Modifier.weight(1f))
            Metric("Received", "%.2f Mbps".format(dashboard.rxMbps), Modifier.weight(1f))
        }
        Metric("Packet loss", "%.2f %%".format(dashboard.lossPct), Modifier.fillMaxWidth())
    }
}

@Composable
private fun Metric(label: String, value: String, modifier: Modifier = Modifier) {
    Column(
        modifier
            .clip(RoundedCornerShape(12.dp))
            .background(Surface2)
            .padding(12.dp),
    ) {
        Text(label.uppercase(), style = MaterialTheme.typography.labelSmall, color = TextMuted)
        Spacer(Modifier.height(2.dp))
        Text(
            text = value,
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
        )
    }
}

@Composable
private fun QualityBadge(quality: LinkQuality) {
    val color = when (quality) {
        LinkQuality.GOOD -> Good
        LinkQuality.DEGRADED -> Degraded
        LinkQuality.POOR -> Poor
        LinkQuality.UNKNOWN -> TextMuted
    }
    Text(
        text = quality.name.lowercase(),
        style = MaterialTheme.typography.labelMedium,
        color = color,
        fontWeight = FontWeight.SemiBold,
    )
}

@Composable
private fun FeatureToggleRow(key: String, label: String, hint: String) {
    Row(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(Surface2)
            .padding(horizontal = 12.dp, vertical = 10.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(
                text = label,
                style = MaterialTheme.typography.titleSmall,
                // Dimmed because these toggles are deliberately inert until the media changes
                // land; an enabled-looking control that does nothing is worse than an obvious
                // placeholder.
                color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.55f),
            )
            Text(text = hint, style = MaterialTheme.typography.bodySmall, color = TextMuted)
        }
        Switch(checked = false, onCheckedChange = null, enabled = false)
    }
}

@Composable
fun EmptyPlaceholder(text: String, modifier: Modifier = Modifier) {
    Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Text(
            text = text,
            color = TextMuted,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(32.dp),
        )
    }
}
