package com.laffy.unifiedstream.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.KeyboardArrowRight
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Computer
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedListItem
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.laffy.unifiedstream.discovery.DiscoveredDevice
import com.laffy.unifiedstream.discovery.DiscoveryState
import com.laffy.unifiedstream.discovery.ManualAddress
import com.laffy.unifiedstream.session.ConnectionState
import com.laffy.unifiedstream.ui.components.Notice
import com.laffy.unifiedstream.ui.components.SectionHeader
import com.laffy.unifiedstream.ui.components.ShapeBadge
import com.laffy.unifiedstream.ui.theme.TechnicalFont

@Composable
fun DevicesDestination(
    devices: List<DiscoveredDevice>,
    discoveryState: DiscoveryState,
    offerManual: Boolean,
    manualError: ManualAddress.Error?,
    connection: ConnectionState,
    onSelect: (DiscoveredDevice) -> Unit,
    onAddManual: (String, String) -> Unit,
    onManualEdited: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var query by remember { mutableStateOf("") }
    val filtered = devices.filter { it.name.contains(query, ignoreCase = true) || it.host.contains(query, ignoreCase = true) }

    DestinationList(modifier) {
        item { SearchField(query) { query = it } }
        item { DiscoveryStatus(discoveryState, devices.size) }
        if (filtered.isEmpty() && discoveryState == DiscoveryState.Found) {
            item { Notice("No devices match your search.") }
        }
        if (filtered.isNotEmpty()) {
            item {
                Column(verticalArrangement = Arrangement.spacedBy(ListItemDefaults.SegmentedGap)) {
                    filtered.forEachIndexed { index, device ->
                        DeviceItem(device, index, filtered.size, connection.isBusy, onSelect)
                    }
                }
            }
        }
        if (offerManual || discoveryState is DiscoveryState.NoNetwork || discoveryState is DiscoveryState.Failed) {
            item { ManualEntry(manualError, connection.isBusy, onAddManual, onManualEdited) }
        }
    }
}

@Composable
private fun SearchField(query: String, onQueryChange: (String) -> Unit) {
    OutlinedTextField(
        value = query,
        onValueChange = onQueryChange,
        label = { Text("Search devices") },
        leadingIcon = { Icon(Icons.Rounded.Search, contentDescription = null) },
        trailingIcon = {
            if (query.isNotEmpty()) {
                IconButton(onClick = { onQueryChange("") }) {
                    Icon(Icons.Rounded.Close, contentDescription = "Clear search")
                }
            }
        },
        singleLine = true,
        shape = CircleShape,
        modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Search nearby computers" },
    )
}

@Composable
private fun DiscoveryStatus(state: DiscoveryState, count: Int) {
    when (state) {
        DiscoveryState.Searching -> Row(
            Modifier.padding(horizontal = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            LoadingIndicator(Modifier.size(40.dp))
            Text("Searching the local network…", color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        DiscoveryState.NoNetwork -> Notice("No Wi-Fi or local network is available. Connect to the same trusted LAN as your PC.", error = true)
        DiscoveryState.TimedOut -> Notice("No computer answered discovery. You can connect by address below.")
        is DiscoveryState.Failed -> Notice(state.message, error = true)
        DiscoveryState.Idle -> Text("Discovery is paused.", color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 8.dp))
        DiscoveryState.Found -> Text(
            "$count computer${if (count == 1) "" else "s"} found",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 8.dp),
        )
    }
}

@Composable
private fun DeviceItem(
    device: DiscoveredDevice,
    index: Int,
    count: Int,
    busy: Boolean,
    onSelect: (DiscoveredDevice) -> Unit,
) {
    SegmentedListItem(
        onClick = { onSelect(device) },
        enabled = device.isCompatible && !busy,
        shapes = ListItemDefaults.segmentedShapes(index = index, count = count),
        colors = ListItemDefaults.segmentedColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainer,
            disabledContainerColor = MaterialTheme.colorScheme.surfaceContainer,
        ),
        verticalAlignment = Alignment.CenterVertically,
        leadingContent = { ShapeBadge(Icons.Rounded.Computer, active = device.isCompatible) },
        supportingContent = {
            Column {
                Text("${device.host}:${device.port}", style = MaterialTheme.typography.bodySmall, fontFamily = TechnicalFont)
                Text(
                    text = if (device.isCompatible) device.caps.joinToString(" · ").ifEmpty { "Compatible" } else "Incompatible protocol v${device.protocolVersion}",
                    style = MaterialTheme.typography.bodySmall,
                    color = if (device.isCompatible) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.error,
                )
            }
        },
        trailingContent = {
            Icon(Icons.AutoMirrored.Rounded.KeyboardArrowRight, contentDescription = null)
        },
        modifier = Modifier.semantics { contentDescription = "Connect to ${device.name}" },
    ) {
        Text(device.name, style = MaterialTheme.typography.titleMediumEmphasized, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
private fun ManualEntry(
    error: ManualAddress.Error?,
    busy: Boolean,
    onAddManual: (String, String) -> Unit,
    onEdited: () -> Unit,
) {
    var host by remember { mutableStateOf("") }
    var port by remember { mutableStateOf("47810") }
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.extraLarge,
        color = MaterialTheme.colorScheme.surfaceContainer,
    ) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            SectionHeader("Connect by address", horizontalPadding = 0.dp)
            OutlinedTextField(
                value = host,
                onValueChange = { host = it; onEdited() },
                label = { Text("IP address or hostname") },
                singleLine = true,
                isError = error == ManualAddress.Error.EMPTY_HOST || error == ManualAddress.Error.MALFORMED_HOST,
                shape = MaterialTheme.shapes.large,
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = port,
                onValueChange = { port = it; onEdited() },
                label = { Text("Control port") },
                singleLine = true,
                isError = error == ManualAddress.Error.MALFORMED_PORT || error == ManualAddress.Error.PORT_OUT_OF_RANGE,
                shape = MaterialTheme.shapes.large,
                modifier = Modifier.fillMaxWidth(),
            )
            error?.let { Text(manualErrorText(it), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
            Button(
                onClick = { onAddManual(host, port) },
                enabled = !busy,
                shapes = ButtonDefaults.shapes(),
                modifier = Modifier.fillMaxWidth(),
            ) { Text("Connect") }
        }
    }
}

private fun manualErrorText(error: ManualAddress.Error): String = when (error) {
    ManualAddress.Error.EMPTY_HOST -> "Enter an address"
    ManualAddress.Error.MALFORMED_HOST -> "That is not a valid address"
    ManualAddress.Error.MALFORMED_PORT -> "Port must be a number"
    ManualAddress.Error.PORT_OUT_OF_RANGE -> "Port must be between 1 and 65535"
}
