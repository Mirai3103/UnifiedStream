# Build and deploy the UnifiedStream Android app over adb.
#
#   make apk        build the debug APK
#   make install    build, then install on the connected device
#   make run        install, then launch the app
#   make log        tail the app's logcat output
#
# Pass DEVICE=<serial> when more than one device is attached (see `adb devices`).

ANDROID_DIR := android
APK := $(ANDROID_DIR)/app/build/outputs/apk/debug/app-debug.apk
PACKAGE := com.laffy.unifiedstream
ACTIVITY := $(PACKAGE)/.MainActivity

# Expands to `-s <serial>` only when DEVICE is set.
ADB := adb $(if $(DEVICE),-s $(DEVICE))

.PHONY: apk install run log uninstall test clean

apk:
	cd $(ANDROID_DIR) && ./gradlew assembleDebug

install: apk
	$(ADB) install -r $(APK)

run: install
	$(ADB) shell am start -n $(ACTIVITY)

log:
	$(ADB) logcat --pid=$$($(ADB) shell pidof -s $(PACKAGE))

uninstall:
	$(ADB) uninstall $(PACKAGE)

test:
	cd $(ANDROID_DIR) && ./gradlew testDebugUnitTest

clean:
	cd $(ANDROID_DIR) && ./gradlew clean
