package app.nexus.transport

data class RadioCapabilities(
    val discoveryOnly: Boolean,
    val metered: Boolean,
    val maxFrameSize: Int,
)

data class DiscoveredEndpoint(
    val serviceId: String,
    val host: String?,
    val port: Int?,
    val bearer: String,
)

interface TransportAdapter {
    val name: String
    val capabilities: RadioCapabilities
    fun startDiscovery(onPeer: (DiscoveredEndpoint) -> Unit)
    fun stopDiscovery()
    fun send(endpoint: DiscoveredEndpoint, frame: ByteArray): ByteArray
}

/** API boundary for a BLE scanner used only for discovery and small handshakes. */
interface BleDiscoveryAdapter : TransportAdapter

/** API 26+ high-bandwidth path. Implement with WifiAwareManager. */
interface WifiAwareAdapter : TransportAdapter

/** Compatibility high-bandwidth path. Implement with WifiP2pManager. */
interface WifiDirectAdapter : TransportAdapter
