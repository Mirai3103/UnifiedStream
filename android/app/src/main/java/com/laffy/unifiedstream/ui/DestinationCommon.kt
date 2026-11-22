package com.laffy.unifiedstream.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.material3.Slider
import androidx.compose.material3.rememberSliderState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.laffy.unifiedstream.session.ConnectionState

/** The scrolling column every destination uses, so insets and rhythm stay identical. */
@Composable
internal fun DestinationList(
    modifier: Modifier = Modifier,
    content: LazyListScope.() -> Unit,
) {
    LazyColumn(
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 16.dp, top = 8.dp, end = 16.dp, bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
        content = content,
    )
}

internal fun connectionTitle(state: ConnectionState): String = when (state) {
    ConnectionState.Idle -> "Not connected"
    ConnectionState.Discovering -> "Looking for a PC"
    is ConnectionState.Connecting -> "Connecting to ${state.peerName}"
    is ConnectionState.Connected -> state.peerName
    is ConnectionState.Reconnecting -> "Reconnecting to ${state.peerName}"
    is ConnectionState.Failed -> "Connection failed"
}

internal fun connectionDetail(state: ConnectionState): String = when (state) {
    ConnectionState.Idle -> "Choose a computer from Devices to start."
    ConnectionState.Discovering -> "Searching this trusted local network."
    is ConnectionState.Connecting -> "Pairing and negotiating available streams…"
    is ConnectionState.Connected -> if (state.negotiatedCaps.isEmpty()) "Session active" else "Streams available: ${state.negotiatedCaps.joinToString(" · ")}"
    is ConnectionState.Reconnecting -> "Attempt ${state.attempt} of ${state.maxAttempts}"
    is ConnectionState.Failed -> state.reason.display
}

/**
 * A slider whose value is owned by the caller. Expressive sliders are driven by a state object,
 * so this keeps that state in step with the externally owned value.
 */
@Composable
internal fun ValueSlider(
    value: Float,
    onValueChange: (Float) -> Unit,
    valueRange: ClosedFloatingPointRange<Float>,
    description: String,
    modifier: Modifier = Modifier,
) {
    val state = rememberSliderState(value = value, trackRange = valueRange)
    LaunchedEffect(value) {
        if (state.value != value) state.value = value
    }
    Slider(
        state = state,
        onValueChange = onValueChange,
        modifier = modifier.semantics { contentDescription = description },
    )
}

/** A one-line connection summary for the title bar, so state is visible on every screen. */
fun connectionSubtitle(state: ConnectionState): String = when (state) {
    is ConnectionState.Connected -> "Connected to ${state.peerName}"
    else -> connectionTitle(state)
}
