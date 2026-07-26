package com.laffy.unifiedstream.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

/**
 * Dark by default and not dynamic-coloured.
 *
 * The product is a dark, minimalist tool; letting the system wallpaper repaint it would make the
 * status colours — good, degraded, poor — unreliable to read at a glance, and reading those at a
 * glance is the screen's whole job.
 */
private val DarkColors = darkColorScheme(
    primary = Accent,
    onPrimary = Color(0xFF06111F),
    primaryContainer = Color(0xFF15304F),
    onPrimaryContainer = Color(0xFFD3E5FF),
    secondary = Color(0xFF9EB2CC),
    onSecondary = Color(0xFF0B1520),
    background = Surface0,
    onBackground = TextPrimary,
    surface = Surface0,
    onSurface = TextPrimary,
    surfaceVariant = Surface1,
    onSurfaceVariant = TextMuted,
    outline = OutlineSubtle,
    error = Poor,
    onError = Color(0xFF1F0708),
)

private val LightColors = lightColorScheme(
    primary = Color(0xFF1D4ED8),
    background = Color(0xFFF6F7FB),
    surface = Color(0xFFFFFFFF),
    error = Color(0xFFB3261E),
)

@Composable
fun UnifiedStreamTheme(
    darkTheme: Boolean = true,
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = if (darkTheme) DarkColors else LightColors,
        typography = Typography,
        content = content,
    )
}
