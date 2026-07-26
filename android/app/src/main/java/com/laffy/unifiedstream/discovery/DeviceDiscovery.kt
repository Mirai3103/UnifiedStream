package com.laffy.unifiedstream.discovery

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.util.Log
import java.util.ArrayDeque
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

private const val TAG = "DeviceDiscovery"

/** What the device list is currently doing, so the UI never shows a bare empty list. */
sealed interface DiscoveryState {
    /** Not browsing. */
    data object Idle : DiscoveryState

    /** No usable network, so browsing cannot even begin. */
    data object NoNetwork : DiscoveryState

    /** Browsing, nothing found yet. */
    data object Searching : DiscoveryState

    /** Browsing found at least one device. */
    data object Found : DiscoveryState

    /** Browsing found nothing within the timeout; the UI offers manual entry. */
    data object TimedOut : DiscoveryState

    /** mDNS itself failed. */
    data class Failed(val message: String) : DiscoveryState
}

/**
 * Browses for `_unifiedstream._udp` desktops on the local network.
 *
 * Holds a Wi-Fi [WifiManager.MulticastLock] for the duration: without it the Wi-Fi driver
 * filters mDNS responses and discovery silently finds nothing at all.
 */
class DeviceDiscovery(
    context: Context,
    private val scope: CoroutineScope,
) {
    private val appContext = context.applicationContext
    private val nsdManager = appContext.getSystemService(NsdManager::class.java)
    private val wifiManager = appContext.getSystemService(WifiManager::class.java)
    private val connectivityManager = appContext.getSystemService(ConnectivityManager::class.java)

    private val _devices = MutableStateFlow<List<DiscoveredDevice>>(emptyList())

    /** Devices found so far, deduplicated by their advertised device id. */
    val devices: StateFlow<List<DiscoveredDevice>> = _devices.asStateFlow()

    private val _state = MutableStateFlow<DiscoveryState>(DiscoveryState.Idle)

    /** Current browse status. */
    val state: StateFlow<DiscoveryState> = _state.asStateFlow()

    private var multicastLock: WifiManager.MulticastLock? = null
    private var discoveryListener: NsdManager.DiscoveryListener? = null
    private var timeoutJob: Job? = null

    /** Serializes resolves: before API 34 a concurrent resolve fails with ALREADY_ACTIVE. */
    private val resolveQueue = ArrayDeque<NsdServiceInfo>()
    private var resolveInFlight = false
    private val resolveLock = Any()

    /** Begin browsing, or report [DiscoveryState.NoNetwork] if there is nothing to browse on. */
    fun start() {
        if (discoveryListener != null) return

        if (!hasUsableNetwork()) {
            _state.value = DiscoveryState.NoNetwork
            return
        }

        acquireMulticastLock()

        val listener = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(serviceType: String) {
                Log.i(TAG, "discovery started for $serviceType")
            }

            override fun onServiceFound(serviceInfo: NsdServiceInfo) {
                enqueueResolve(serviceInfo)
            }

            override fun onServiceLost(serviceInfo: NsdServiceInfo) {
                val name = serviceInfo.serviceName ?: return
                _devices.value = removeByInstanceName(_devices.value, name)
                refreshFoundState()
            }

            override fun onDiscoveryStopped(serviceType: String) {
                Log.i(TAG, "discovery stopped for $serviceType")
            }

            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.w(TAG, "start discovery failed: $errorCode")
                _state.value = DiscoveryState.Failed("could not start discovery (code $errorCode)")
                stop()
            }

            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.w(TAG, "stop discovery failed: $errorCode")
            }
        }

        discoveryListener = listener
        _state.value = DiscoveryState.Searching

        try {
            nsdManager.discoverServices(
                com.laffy.unifiedstream.protocol.SERVICE_TYPE,
                NsdManager.PROTOCOL_DNS_SD,
                listener,
            )
        } catch (e: IllegalArgumentException) {
            Log.w(TAG, "discoverServices rejected", e)
            discoveryListener = null
            _state.value = DiscoveryState.Failed(e.message ?: "discovery rejected")
            releaseMulticastLock()
            return
        }

        startTimeoutWatch()
    }

    /** Stop browsing and release the multicast lock. */
    fun stop() {
        timeoutJob?.cancel()
        timeoutJob = null

        discoveryListener?.let { listener ->
            runCatching { nsdManager.stopServiceDiscovery(listener) }
                .onFailure { Log.w(TAG, "stopServiceDiscovery failed", it) }
        }
        discoveryListener = null

        synchronized(resolveLock) {
            resolveQueue.clear()
            resolveInFlight = false
        }

        releaseMulticastLock()

        // Manual entries are the user's explicit intent and survive a browse restart.
        _devices.value = _devices.value.filter { it.manual }
        if (_state.value !is DiscoveryState.Failed) {
            _state.value = DiscoveryState.Idle
        }
    }

    /** Add a peer the user typed in by hand. */
    fun addManual(device: DiscoveredDevice) {
        _devices.value = mergeDiscovered(_devices.value.filterNot { it.id == device.id }, device)
        refreshFoundState()
    }

    // --- Resolution ------------------------------------------------------------------------

    private fun enqueueResolve(serviceInfo: NsdServiceInfo) {
        synchronized(resolveLock) {
            resolveQueue.addLast(serviceInfo)
            if (resolveInFlight) return
            resolveInFlight = true
        }
        drainResolveQueue()
    }

    private fun drainResolveQueue() {
        val next = synchronized(resolveLock) {
            val head = resolveQueue.pollFirst()
            if (head == null) {
                resolveInFlight = false
            }
            head
        } ?: return

        resolve(next) {
            drainResolveQueue()
        }
    }

    private fun resolve(serviceInfo: NsdServiceInfo, onDone: () -> Unit) {
        @Suppress("DEPRECATION")
        val listener = object : NsdManager.ResolveListener {
            override fun onResolveFailed(failed: NsdServiceInfo, errorCode: Int) {
                Log.w(TAG, "resolve failed for ${failed.serviceName}: $errorCode")
                onDone()
            }

            override fun onServiceResolved(resolved: NsdServiceInfo) {
                onResolvedService(resolved)
                onDone()
            }
        }

        try {
            @Suppress("DEPRECATION")
            nsdManager.resolveService(serviceInfo, listener)
        } catch (e: IllegalArgumentException) {
            Log.w(TAG, "resolveService rejected", e)
            onDone()
        }
    }

    private fun onResolvedService(resolved: NsdServiceInfo) {
        val host = hostAddressOf(resolved) ?: return
        val device = parseTxtRecord(
            attributes = resolved.attributes.orEmpty(),
            instanceName = resolved.serviceName.orEmpty(),
            host = host,
            port = resolved.port,
        ) ?: run {
            Log.w(TAG, "ignoring ${resolved.serviceName}: incomplete TXT record")
            return
        }

        _devices.value = mergeDiscovered(_devices.value, device)
        refreshFoundState()
    }

    /**
     * Pick a usable address for a resolved service.
     *
     * A link-local IPv6 literal (`fe80::/10`) is unusable without its scope id, and the scope
     * id is an interface name that does not survive being passed around as a host string — so
     * such an address is rejected outright rather than stripped and handed to `connect()`,
     * which would fail at tap time with no explanation. Returning null drops the record; the
     * device reappears when a usable address resolves.
     */
    private fun hostAddressOf(info: NsdServiceInfo): String? {
        val candidates: List<java.net.InetAddress> =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                info.hostAddresses
            } else {
                @Suppress("DEPRECATION")
                listOfNotNull(info.host)
            }

        val usable = candidates.filterNot { it.isLinkLocalAddress || it.isAnyLocalAddress }
        // IPv4 first: it is what a home LAN actually routes, and it is what users recognise.
        val preferred = usable.firstOrNull { it is java.net.Inet4Address } ?: usable.firstOrNull()

        return preferred?.hostAddress?.substringBefore('%')?.takeIf { it.isNotBlank() }
    }

    // --- State bookkeeping -----------------------------------------------------------------

    private fun refreshFoundState() {
        val current = _state.value
        if (current is DiscoveryState.Failed || current is DiscoveryState.NoNetwork) return
        if (_devices.value.isNotEmpty()) {
            _state.value = DiscoveryState.Found
        } else if (current is DiscoveryState.Found) {
            _state.value = DiscoveryState.Searching
        }
    }

    private fun startTimeoutWatch() {
        timeoutJob?.cancel()
        timeoutJob = scope.launch {
            delay(ManualAddress.DISCOVERY_TIMEOUT_MS)
            if (_devices.value.isEmpty() && _state.value is DiscoveryState.Searching) {
                _state.value = DiscoveryState.TimedOut
            }
        }
    }

    private fun hasUsableNetwork(): Boolean {
        val network = connectivityManager.activeNetwork ?: return false
        val caps = connectivityManager.getNetworkCapabilities(network) ?: return false
        // Cellular cannot reach a PC on the LAN, so it does not count as usable here — this
        // deliberately checks transports rather than NET_CAPABILITY_INTERNET, which mobile
        // data also satisfies.
        return caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) ||
            caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
    }

    private fun acquireMulticastLock() {
        if (multicastLock != null) return
        multicastLock = runCatching {
            wifiManager.createMulticastLock("unifiedstream-discovery").apply {
                setReferenceCounted(false)
                acquire()
            }
        }.onFailure { Log.w(TAG, "could not acquire multicast lock", it) }.getOrNull()
    }

    private fun releaseMulticastLock() {
        multicastLock?.let { lock ->
            runCatching { if (lock.isHeld) lock.release() }
                .onFailure { Log.w(TAG, "could not release multicast lock", it) }
        }
        multicastLock = null
    }
}
