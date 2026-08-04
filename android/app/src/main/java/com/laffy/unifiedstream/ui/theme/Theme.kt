package com.laffy.unifiedstream.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

private val DarkColors = darkColorScheme(
    primary = LuminousMint,
    onPrimary = DeepMintInk,
    primaryContainer = TealContainer,
    onPrimaryContainer = PaleMint,
    secondary = PaleSage,
    onSecondary = DeepMintInk,
    secondaryContainer = MutedSage,
    onSecondaryContainer = PaleSage,
    background = DeepForest,
    onBackground = SoftPorcelain,
    surface = DeepForest,
    onSurface = SoftPorcelain,
    surfaceVariant = TonalForest,
    onSurfaceVariant = SageGray,
    surfaceContainer = TonalForest,
    surfaceContainerHigh = RaisedForest,
    surfaceContainerHighest = HighestForest,
    outline = MineralOutline,
    outlineVariant = QuietOutline,
    error = SoftErrorCoral,
    onError = Color(0xFF690005),
)

private val LightColors = lightColorScheme(
    primary = GroundedTeal,
    onPrimary = Color.White,
    primaryContainer = PaleMint,
    onPrimaryContainer = Color(0xFF00201C),
    secondary = SlateSage,
    onSecondary = Color.White,
    secondaryContainer = PaleSage,
    onSecondaryContainer = Color(0xFF062019),
    background = MintedWhite,
    onBackground = ForestInk,
    surface = MintedWhite,
    onSurface = ForestInk,
    surfaceVariant = SoftSage,
    onSurfaceVariant = SlateSage,
    surfaceContainer = SoftSage,
    surfaceContainerHigh = RaisedSage,
    surfaceContainerHighest = HighestSage,
    outline = Color(0xFF6F7977),
    outlineVariant = Color(0xFFBFC9C6),
    error = MaterialError,
    onError = Color.White,
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
