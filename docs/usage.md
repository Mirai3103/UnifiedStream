# Using UnifiedStream

Complete [Linux installation and system setup](linux-installation.md) before following this guide.

## Connect the devices

1. Connect the Linux PC and Android phone to the same trusted LAN. Disable VPN routing and guest-network client isolation for the first test.
2. Launch UnifiedStream on Linux. The **This device** panel should show advertising enabled, control port `47810`, and the active media port.
3. Open UnifiedStream on Android and grant notification permission when requested so the foreground session remains visible.
4. Tap the discovered PC. If it does not appear after ten seconds, enter the PC's LAN address and port `47810` in **Connect by address**.
5. Compare the device name shown on both devices and select **Allow** in the desktop pairing prompt.
6. Confirm Android shows a connected state and the desktop dashboard shows live telemetry.

## Use the phone as a webcam

1. Ensure `v4l2loopback` is loaded and list the virtual cameras:

   ```bash
   v4l2-ctl --list-devices
   ```

2. On Android, enable **Camera** and grant camera permission.
3. Choose the front or back camera and the desired supported resolution.
4. On Linux, confirm the desktop Camera card becomes active and shows frame statistics.
5. Open the target application and choose **UnifiedStream Camera** as its camera. Applications opened before the virtual device appeared may need to be restarted.

Stop Camera in Android before unloading `v4l2loopback` or quitting the desktop app.

## Use the phone as a microphone

1. On Android, enable **Microphone** and grant microphone permission.
2. Confirm Android shows the microphone foreground indicator.
3. On Linux, find **UnifiedStream Microphone** under Sources:

   ```bash
   wpctl status
   ```

4. Select **UnifiedStream Microphone** in the conferencing, recording, or streaming application.
5. Speak into the phone and confirm the level meter moves in both UnifiedStream and the target application.

## Play Linux audio on the phone

1. Enable **Speaker** on Android.
2. Enable the Speaker stream on the desktop.
3. Select **UnifiedStream Speaker** as an output for an application with the desktop audio settings, or use the desktop routing control when available.
4. Play audio and confirm the Android output meter moves.
5. Adjust volume or mute from Android. Use headphones to prevent acoustic feedback when the microphone is also active.

When finished, restore the previous Linux output before stopping the stream. UnifiedStream attempts to restore stale routing after a restart, but verifying the selected output avoids silence in other applications.

## Disconnect safely

1. Stop Camera, Microphone, and Speaker.
2. Restore the normal Linux input and output devices in other applications.
3. Disconnect the session from either app.
4. Quit the desktop app before removing the virtual camera module.

See [Troubleshooting](troubleshooting.md) when discovery, connection, or a virtual device does not behave as described.
