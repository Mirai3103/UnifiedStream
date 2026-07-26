package com.laffy.unifiedstream.ui

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.laffy.unifiedstream.discovery.DeviceDiscovery
import com.laffy.unifiedstream.discovery.DeviceIdentity
import com.laffy.unifiedstream.discovery.DeviceIdentityStore
import com.laffy.unifiedstream.discovery.DiscoveredDevice
import com.laffy.unifiedstream.discovery.DiscoveryState
import com.laffy.unifiedstream.discovery.ManualAddress
import com.laffy.unifiedstream.protocol.Caps
import com.laffy.unifiedstream.session.ConnectionState
import com.laffy.unifiedstream.session.SessionManager
import com.laffy.unifiedstream.telemetry.LinkQuality
import com.laffy.unifiedstream.transport.TestStreamConfig
import com.laffy.unifiedstream.transport.TestStreamReport
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

/** Everything the dashboard renders. */
data class DashboardState(
    val rttMs: Double? = null,
    val txMbps: Double = 0.0,
    val rxMbps: Double = 0.0,
    val lossPct: Double = 0.0,
    val jitterMs: Double = 0.0,
    val quality: LinkQuality = LinkQuality.UNKNOWN,
)

/**
 * Bridges discovery, session, and telemetry to Compose state.
 *
 * Every screen renders from these flows rather than touching sockets, so the UI cannot drift
 * from what the connection is actually doing.
 */
class UnifiedStreamViewModel(application: Application) : AndroidViewModel(application) {

    private val identityStore = DeviceIdentityStore(application)
    private val discovery = DeviceDiscovery(application, viewModelScope)

    private val _identity = MutableStateFlow<DeviceIdentity?>(null)

    /** This phone's stable identity, once loaded. */
    val identity: StateFlow<DeviceIdentity?> = _identity.asStateFlow()

    private var session: SessionManager? = null

    private val _connection = MutableStateFlow<ConnectionState>(ConnectionState.Idle)

    /** Connection lifecycle. The single source of truth for connection UI. */
    val connection: StateFlow<ConnectionState> = _connection.asStateFlow()

    private val _dashboard = MutableStateFlow(DashboardState())

    /** Live link-quality metrics. */
    val dashboard: StateFlow<DashboardState> = _dashboard.asStateFlow()

    private val _manualError = MutableStateFlow<ManualAddress.Error?>(null)

    /** Validation error for the manual-entry form, if any. */
    val manualError: StateFlow<ManualAddress.Error?> = _manualError.asStateFlow()

    private val _testReport = MutableStateFlow<TestStreamReport?>(null)

    /** Synthetic test-stream counters. */
    val testReport: StateFlow<TestStreamReport?> = _testReport.asStateFlow()

    private val _testRunning = MutableStateFlow(false)

    /** Whether the synthetic test stream is sending. */
    val testRunning: StateFlow<Boolean> = _testRunning.asStateFlow()

    /** Devices found on the network. */
    val devices: StateFlow<List<DiscoveredDevice>> = discovery.devices

    /** Browse status, so the UI never shows a bare empty list. */
    val discoveryState: StateFlow<DiscoveryState> = discovery.state

    /** Whether the manual-entry affordance should be offered. */
    val offerManualEntry: StateFlow<Boolean> = combine(
        discovery.state,
        discovery.devices,
    ) { state, found ->
        state is DiscoveryState.TimedOut || (state is DiscoveryState.Failed && found.isEmpty())
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), false)

    init {
        viewModelScope.launch {
            val loaded = identityStore.loadOrCreate()
            _identity.value = loaded

            val manager = SessionManager(
                scope = viewModelScope,
                deviceId = loaded.id,
                deviceName = loaded.name,
                caps = Caps.ALL,
            )
            session = manager

            launch { manager.state.collect { _connection.value = it } }
            launch { manager.testReport.collect { _testReport.value = it } }
            launch { manager.testRunning.collect { _testRunning.value = it } }
            launch {
                combine(manager.rttMs, manager.peerTelemetry) { rtt, peer ->
                    DashboardState(
                        rttMs = rtt,
                        // The desktop's tx is our rx and vice versa; showing it from the peer's
                        // report is the only way we see the downstream rate.
                        txMbps = peer.rxMbps,
                        rxMbps = peer.txMbps,
                        lossPct = peer.lossPct,
                        jitterMs = peer.jitterMs,
                        quality = LinkQuality.classify(rtt, peer.lossPct),
                    )
                }.collect { _dashboard.value = it }
            }
        }
    }

    /** Begin browsing for desktops. */
    fun startDiscovery() {
        discovery.start()
        session?.enterDiscovery()
    }

    /** Stop browsing. */
    fun stopDiscovery() {
        discovery.stop()
    }

    /** Connect to a discovered or manually entered device. */
    fun connect(device: DiscoveredDevice) {
        session?.connect(device)
    }

    /** End the session cleanly. */
    fun disconnect() {
        session?.disconnect()
        _dashboard.value = DashboardState()
    }

    /**
     * Start the synthetic test stream.
     *
     * No codecs exist yet, so this is what proves the transport end to end.
     */
    fun startTestStream(rateHz: Int, frameBytes: Int) {
        session?.startTestStream(TestStreamConfig(rateHz = rateHz, frameBytes = frameBytes))
    }

    /** Stop the synthetic test stream. */
    fun stopTestStream() {
        session?.stopTestStream()
    }

    /** Stop retrying and go back to idle. */
    fun cancelReconnect() {
        session?.cancelReconnect()
    }

    /** Validate and add a hand-typed peer, returning true when it was accepted. */
    fun addManualDevice(host: String, port: String): Boolean =
        when (val result = ManualAddress.validate(host, port)) {
            is ManualAddress.Validation.Valid -> {
                _manualError.value = null
                val device = ManualAddress.toDevice(result.host, result.port)
                discovery.addManual(device)
                connect(device)
                true
            }

            is ManualAddress.Validation.Invalid -> {
                _manualError.value = result.error
                false
            }
        }

    /** Clear a stale validation error when the user edits the form. */
    fun clearManualError() {
        _manualError.value = null
    }

    override fun onCleared() {
        super.onCleared()
        discovery.stop()
        session?.disconnect()
    }
}
