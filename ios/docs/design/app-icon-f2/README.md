# Studio TV F2

The shipping editable icon is `../../../FamilyStream/Resources/AppIcon.icon`, copied from the final document saved in Apple Icon Composer. `production/StudioTV.icon` retains that authoring snapshot. Future production edits should use `AppIcon.icon`. The four full-canvas SVG sources in `production/layers/` have transparent surroundings and no baked background, bevel, or shadow.

Groups, back to front:

1. TV: screen and foot, Combined material boundary.
2. Playback marks: play symbol and status indicator, Individual material boundaries.

Both groups have translucency disabled. Blur and refraction are disabled in the observed Composer inspector. Mono-only neutral shadow strengths are 20% for TV and 10% for playback marks; verified in the Composer-saved document. Other appearances retain their existing material values.

The user set Default and Dark foreground colors and canvas gradients. The user subsequently reserved all color editing, including Mono. No further appearance colors should be changed without their instruction.

Status on 2026-10-19: the user completed Default, Dark, and Mono colors and backgrounds and approved installation. The saved document includes all four layers and the three appearance configurations. The iOS target includes `AppIcon.icon` and retains `ASSETCATALOG_COMPILER_APPICON_NAME = AppIcon` for Debug and Release. The previous asset catalog remains intact.

The signed Debug device build succeeded. Build output confirms actool compiled `AppIcon.icon` with `--app-icon AppIcon`; the generated Info.plist identifies AppIcon. Installation over the existing `com.streamtv.familystream` app succeeded on connected iOS device, without uninstalling. Build log: `/tmp/family-stream-icon-build.log`. No tests or playback probes were run. Final Home Screen appearance and Clear/Tinted visual acceptance on the phone remain for user confirmation.
