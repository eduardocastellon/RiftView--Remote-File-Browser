// Error messages
export const enum PairStatus {
    IDLE, // Not yet started searching or pairing
    SCANNING, // Actively searching/discovering the device on the network
    CONNECTING, // Found the device, actively opening a connection/handshake
    AUTHENTICATING, // Waiting for credentials, PIN, or user confirmation
    SUCCESS, // Device found and connected successfully
    FAILED, // Device exists but failed to connect
    TIMEOUT, // Connection or pairing attempt timed out
    NOT_FOUND, // No such device exists or was found
    DISCONNECTED, // Previously connected, but the connection dropped
    RECONNECTING, // Automatically trying to re-establish a lost connection
    BUSY, // Device is busy or already paired with another host
}