package com.laffy.unifiedstream.discovery

import android.content.Context
import android.os.Build
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import java.util.UUID
import kotlinx.coroutines.flow.first

/** Who this phone says it is, stable across restarts. */
data class DeviceIdentity(
    val id: String,
    val name: String,
)

private val Context.identityDataStore: DataStore<Preferences> by preferencesDataStore(
    name = "device_identity",
)

/**
 * Generate-once-then-load device identity, mirroring the desktop's `DeviceIdentity`.
 *
 * The peer records this id as trusted after pairing, so regenerating it would silently force
 * the user to re-approve the phone.
 */
class DeviceIdentityStore(private val context: Context) {

    private val idKey = stringPreferencesKey("device_id")
    private val nameKey = stringPreferencesKey("device_name")

    /** Load the persisted identity, creating and storing one on first run. */
    suspend fun loadOrCreate(): DeviceIdentity {
        // A single edit() transaction, not read-then-write: two coroutines racing on first
        // launch would otherwise each generate a UUID and one would win arbitrarily.
        val prefs = context.identityDataStore.edit { prefs ->
            if (prefs[idKey].isNullOrBlank()) {
                prefs[idKey] = UUID.randomUUID().toString()
            }
            if (prefs[nameKey].isNullOrBlank()) {
                prefs[nameKey] = defaultDeviceName()
            }
        }
        return DeviceIdentity(
            id = prefs[idKey].orEmpty(),
            name = prefs[nameKey] ?: defaultDeviceName(),
        )
    }

    /** Rename this device without disturbing its identity. */
    suspend fun rename(name: String) {
        val trimmed = name.trim().ifEmpty { defaultDeviceName() }
        context.identityDataStore.edit { it[nameKey] = trimmed }
    }

    /** The current identity, or null if one has never been generated. */
    suspend fun peek(): DeviceIdentity? {
        val prefs = context.identityDataStore.data.first()
        val id = prefs[idKey]?.takeIf { it.isNotBlank() } ?: return null
        return DeviceIdentity(id = id, name = prefs[nameKey] ?: defaultDeviceName())
    }

    private fun defaultDeviceName(): String =
        listOf(Build.MANUFACTURER, Build.MODEL)
            .filter { it.isNotBlank() }
            .joinToString(" ")
            .trim()
            .ifEmpty { "Android phone" }
}
