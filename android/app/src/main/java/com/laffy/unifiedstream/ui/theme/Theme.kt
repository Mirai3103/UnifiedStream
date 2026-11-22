package com.laffy.unifiedstream.ui.theme

import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MotionScheme
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
    tertiary = Degraded,
    onTertiary = Color(0xFF3F2E00),
    tertiaryContainer = Color(0xFF5B4300),
    onTertiaryContainer = Color(0xFFFFDEA6),
    background = DeepForest,
    onBackground = SoftPorcelain,
    surface = DeepForest,
    onSurface = SoftPorcelain,
    surfaceVariant = TonalForest,
    onSurfaceVariant = SageGray,
    surfaceContainerLowest = Color(0xFF090F0E),
    surfaceContainerLow = Color(0xFF131918),
    surfaceContainer = TonalForest,
    surfaceContainerHigh = RaisedForest,
    surfaceContainerHighest = HighestForest,
    outline = MineralOutline,
    outlineVariant = QuietOutline,
    error = SoftErrorCoral,
    onError = Color(0xFF690005),
    errorContainer = Color(0xFF93000A),
    onErrorContainer = Color(0xFFFFDAD6),
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
    tertiary = Color(0xFF7B5800),
    onTertiary = Color.White,
    tertiaryContainer = Color(0xFFFFDEA6),
    onTertiaryContainer = Color(0xFF271900),
    background = MintedWhite,
    onBackground = ForestInk,
    surface = MintedWhite,
    onSurface = ForestInk,
    surfaceVariant = SoftSage,
    onSurfaceVariant = SlateSage,
    surfaceContainerLowest = Color.White,
    surfaceContainerLow = Color(0xFFEFF5F3),
    surfaceContainer = SoftSage,
    surfaceContainerHigh = RaisedSage,
    surfaceContainerHighest = HighestSage,
    outline = Color(0xFF6F7977),
    outlineVariant = Color(0xFFBFC9C6),
    error = MaterialError,
    onError = Color.White,
    errorContainer = Color(0xFFFFDAD6),
    onErrorContainer = Color(0xFF410002),
)

/**
 * Material 3 Expressive theme with the fixed UnifiedStream mint/teal palette.
 *
 * Expressive contributes the spring-based motion scheme, shape scale, and emphasized type
 * roles. The palette is deliberately not dynamic: mint means connected or live throughout
 * the product.
 */
@Composable
fun UnifiedStreamTheme(
    darkTheme: Boolean = true,
    content: @Composable () -> Unit,
) {
    MaterialExpressiveTheme(
        colorScheme = if (darkTheme) DarkColors else LightColors,
        motionScheme = MotionScheme.expressive(),
        shapes = MaterialTheme.shapes,
        typography = Typography,
        content = content,
    )
}
