package com.laffy.unifiedstream

import org.junit.Assert.assertEquals
import org.junit.Test

class NavigationStateTest {
    @Test
    fun launch_connect_camera_and_settings_follow_stack_order() {
        var stack = listOf(AppDestination.DEVICES.name)
        stack = pushDestination(stack, AppDestination.HOME)
        stack = pushDestination(stack, AppDestination.CAMERA)
        stack = pushDestination(stack, AppDestination.SETTINGS)

        assertEquals(listOf("DEVICES", "HOME", "CAMERA", "SETTINGS"), stack)
        stack = popDestination(stack)
        assertEquals(AppDestination.CAMERA.name, stack.last())
        stack = popDestination(stack)
        assertEquals(AppDestination.HOME.name, stack.last())
    }

    @Test
    fun back_at_devices_root_keeps_root_for_activity_to_exit() {
        val root = listOf(AppDestination.DEVICES.name)
        assertEquals(root, popDestination(root))
    }

    @Test
    fun pushing_current_destination_does_not_duplicate_it() {
        val home = listOf(AppDestination.DEVICES.name, AppDestination.HOME.name)
        assertEquals(home, pushDestination(home, AppDestination.HOME))
    }
}
