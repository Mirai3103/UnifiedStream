package com.laffy.unifiedstream.ui.components

import android.provider.Settings
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.HelpOutline
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.WarningAmber
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ButtonGroupDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.laffy.unifiedstream.telemetry.LinkQuality
import com.laffy.unifiedstream.ui.theme.TechnicalFont

/**
 * Whether the system allows animation. When the user turns animations off, wavy and morphing
 * indicators fall back to static shapes; every state they show is also carried by text and icons.
 */
@Composable
fun rememberAnimationsEnabled(): Boolean {
    val context = LocalContext.current
    return remember(context) {
        Settings.Global.getFloat(context.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) != 0f
    }
}

/** A short caps-free label that introduces a group of cards, with an optional trailing element. */
@Composable
fun SectionHeader(
    text: String,
    modifier: Modifier = Modifier,
    horizontalPadding: Dp = 8.dp,
    trailing: @Composable (() -> Unit)? = null,
) {
    Row(
        modifier = modifier.fillMaxWidth().padding(horizontal = horizontalPadding),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.titleMediumEmphasized,
            color = MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.weight(1f),
        )
        trailing?.invoke()
    }
}

/** A circular-to-cookie badge that holds an icon and signals on/off through shape and tone. */
@Composable
fun ShapeBadge(
    icon: ImageVector,
    active: Boolean,
    modifier: Modifier = Modifier,
    size: Dp = 48.dp,
) {
    val container by animateColorAsState(
        if (active) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.surfaceContainerHighest,
        animationSpec = MaterialTheme.motionScheme.defaultEffectsSpec(),
        label = "badge container",
    )
    val content by animateColorAsState(
        if (active) MaterialTheme.colorScheme.onPrimary else MaterialTheme.colorScheme.onSurfaceVariant,
        animationSpec = MaterialTheme.motionScheme.defaultEffectsSpec(),
        label = "badge content",
    )
    val cookie = MaterialShapes.Cookie9Sided.toShape()
    val shape: Shape = if (active) cookie else CircleShape
    Box(
        modifier = modifier.size(size).clip(shape).background(container),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, contentDescription = null, tint = content, modifier = Modifier.size(size / 2))
    }
}

/**
 * A card for one stream (camera, microphone, speaker): a badge, title and status line, an
 * on/off toggle button, and the stream's own controls underneath.
 */
@Composable
fun StreamCard(
    title: String,
    status: String,
    icon: ImageVector,
    checked: Boolean,
    pending: Boolean,
    toggleDescription: String,
    toggleEnabled: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    statusIsError: Boolean = false,
    content: @Composable ColumnScope.() -> Unit = {},
) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.extraLarge,
        color = MaterialTheme.colorScheme.surfaceContainer,
    ) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                ShapeBadge(icon, active = checked && !pending)
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(title, style = MaterialTheme.typography.titleLargeEmphasized)
                    Text(
                        text = status,
                        style = MaterialTheme.typography.bodyMedium,
                        color = if (statusIsError) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                StreamToggle(checked, pending, toggleDescription, toggleEnabled, onCheckedChange)
            }
            content()
        }
    }
}

@Composable
private fun StreamToggle(
    checked: Boolean,
    pending: Boolean,
    description: String,
    enabled: Boolean,
    onCheckedChange: (Boolean) -> Unit,
) {
    val stateText = if (pending) "Starting" else if (checked) "On" else "Off"
    ToggleButton(
        checked = checked,
        onCheckedChange = onCheckedChange,
        enabled = enabled,
        modifier = Modifier.semantics {
            contentDescription = description
            stateDescription = stateText
        },
    ) {
        if (pending) {
            LoadingIndicator(Modifier.size(ButtonDefaults.IconSize))
        }
        Text(
            text = if (pending) "Starting" else if (checked) "On" else "Off",
            modifier = Modifier.padding(start = if (pending) 8.dp else 0.dp),
        )
    }
}

/** A labelled measurement. Values use the technical font so digits do not jitter. */
@Composable
fun MetricTile(
    label: String,
    value: String,
    modifier: Modifier = Modifier,
    shape: Shape = MaterialTheme.shapes.large,
) {
    Surface(modifier = modifier, shape = shape, color = MaterialTheme.colorScheme.surfaceContainerHigh) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            // No maxLines: at large font scales the unit must wrap rather than be clipped away.
            Text(value, style = MaterialTheme.typography.titleLargeEmphasized, fontFamily = TechnicalFont)
        }
    }
}

/** Link quality as an icon plus word, so the state is never colour-only. */
@Composable
fun QualityPill(quality: LinkQuality, modifier: Modifier = Modifier) {
    val colors = MaterialTheme.colorScheme
    val (container, content, icon) = when (quality) {
        LinkQuality.GOOD -> Triple(colors.primaryContainer, colors.onPrimaryContainer, Icons.Rounded.CheckCircle)
        LinkQuality.DEGRADED -> Triple(colors.tertiaryContainer, colors.onTertiaryContainer, Icons.Rounded.WarningAmber)
        LinkQuality.POOR -> Triple(colors.errorContainer, colors.onErrorContainer, Icons.Rounded.ErrorOutline)
        LinkQuality.UNKNOWN -> Triple(colors.surfaceContainerHighest, colors.onSurfaceVariant, Icons.AutoMirrored.Rounded.HelpOutline)
    }
    Surface(modifier = modifier, shape = CircleShape, color = container, contentColor = content) {
        Row(
            Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Icon(icon, contentDescription = null, modifier = Modifier.size(16.dp))
            Text(quality.name.lowercase().replaceFirstChar { it.uppercase() }, style = MaterialTheme.typography.labelLarge)
        }
    }
}

/** An audio level as a wavy bar. The wave flattens when the stream is muted or animations are off. */
@Composable
fun LevelMeter(
    level: Float,
    label: String,
    live: Boolean,
    modifier: Modifier = Modifier,
) {
    val clamped = level.coerceIn(0f, 1f)
    val animated by animateFloatAsState(
        targetValue = clamped,
        animationSpec = MaterialTheme.motionScheme.defaultEffectsSpec(),
        label = "$label animation",
    )
    val waves = live && rememberAnimationsEnabled()
    val percent = (clamped * 100).toInt()
    LinearWavyProgressIndicator(
        progress = { animated },
        amplitude = { if (waves) 1f else 0f },
        modifier = modifier.fillMaxWidth().semantics { contentDescription = "$label $percent percent" },
    )
}

/** A message with an icon; errors use the error container and info uses a neutral tonal one. */
@Composable
fun Notice(
    text: String,
    modifier: Modifier = Modifier,
    error: Boolean = false,
) {
    val colors = MaterialTheme.colorScheme
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.large,
        color = if (error) colors.errorContainer else colors.surfaceContainerHigh,
        contentColor = if (error) colors.onErrorContainer else colors.onSurfaceVariant,
    ) {
        Row(
            Modifier.padding(16.dp),
            verticalAlignment = Alignment.Top,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Icon(
                imageVector = if (error) Icons.Rounded.ErrorOutline else Icons.Rounded.Info,
                contentDescription = if (error) "Error" else null,
                modifier = Modifier.size(20.dp),
            )
            Text(text, style = MaterialTheme.typography.bodyMedium)
        }
    }
}

/**
 * A row of connected toggle buttons where exactly one option is selected, such as resolution.
 */
@Composable
fun <T> ConnectedChoiceGroup(
    options: List<T>,
    selected: T,
    label: (T) -> String,
    onSelect: (T) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    Row(
        modifier = modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(ButtonGroupDefaults.ConnectedSpaceBetween),
    ) {
        options.forEachIndexed { index, option ->
            val shapes = when (index) {
                0 -> ButtonGroupDefaults.connectedLeadingButtonShapes()
                options.lastIndex -> ButtonGroupDefaults.connectedTrailingButtonShapes()
                else -> ButtonGroupDefaults.connectedMiddleButtonShapes()
            }
            ToggleButton(
                checked = option == selected,
                onCheckedChange = { if (it) onSelect(option) },
                enabled = enabled,
                shapes = shapes,
                modifier = Modifier.weight(1f).semantics { role = Role.RadioButton },
            ) {
                Text(label(option))
            }
        }
    }
}
