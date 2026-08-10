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

### On a Windows build

Windows has no kernel module to load. The virtual camera is a DirectShow filter each application loads for itself, registered once from an elevated prompt with the `regsvr32` command the desktop's Camera card names — one for 64-bit applications and one for 32-bit ones, because an application can only load a filter of its own architecture. The desktop refuses to start the camera, and names the missing command, until both are registered.

The coverage limitation is the Windows equivalent of the `v4l2loopback` prerequisite, and it is worth knowing before you go looking for a setting that does not exist:

- **Applications that enumerate DirectShow devices see the camera.** Zoom, Discord, OBS, Skype, and Chromium-based browsers, including Chrome and Edge.
- **UWP and Microsoft Store applications, and applications that use Media Foundation exclusively, do not.** The camera will not appear in their device lists at all. This is a property of how those applications enumerate cameras, not of the installation, and no amount of re-registering changes it.

Applications opened before the filter was registered need restarting, as they do on Linux.

## Use the phone as a microphone

1. On Android, enable **Microphone** and grant microphone permission.
2. Confirm Android shows the microphone foreground indicator.
3. On Linux, find **UnifiedStream Microphone** under Sources:

   ```bash
   wpctl status
   ```

4. Select **UnifiedStream Microphone** in the conferencing, recording, or streaming application.
5. Speak into the phone and confirm the level meter moves in both UnifiedStream and the target application.

### On a Windows build

Windows presents the microphone through **VB-CABLE**, a virtual audio cable by VB-Audio that is installed alongside UnifiedStream. The desktop plays the phone's audio into the cable's playback half and applications record from its capture half, so the device to select is:

**CABLE Output (VB-Audio Virtual Cable)**

Three differences from Linux follow from the cable belonging to a driver rather than to this application, and are worth knowing before they look like faults:

- **The device is always there.** It is created when VB-CABLE is installed, not when a stream starts, so it appears in every application's microphone list whether or not UnifiedStream is running. With no stream it carries silence.
- **It does not carry this product's name.** The cable is VB-Audio's, and so is the name.
- **The playback half appears in your output list too.** `CABLE Input`, and on some releases `CABLE In 16ch`, show up alongside your real speakers. Leave them alone: selecting one as your system output sends your PC's sound into the phone's microphone path instead of to your speakers, and the Speaker stream refuses to start while that is the case. See the [troubleshooting note](troubleshooting.md#extra-vb-audio-playback-devices-appear-on-windows).

If VB-CABLE is not installed — an unpackaged build, or a manual uninstall — the desktop refuses the microphone and names what is missing. The Camera and Speaker streams are unaffected.

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
