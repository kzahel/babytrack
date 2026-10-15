# Android store assets

Public name: **Lantern Baby Tracker**, per the
[interface topic](../../../docs/topics/interface-design-and-localization.md).
The application ID stays `org.babytrack.app`.

`icon.svg` reproduces the existing adaptive launcher mark and colors.
`feature-graphic.svg` is the corresponding 1024 × 500 store graphic.
Render each SVG in a browser at its declared size and export as an opaque PNG
for Play. The first draft used bundled Playwright Chromium; no image-model
or third-party art was used. Keep editable vector sources here.

Phone screenshots come from the source-matched CI artifact
`babytrack-android-screen-gallery`, using the fictional `today-typical`,
`history-typical`, `capture-bottle`, and `family-local` light/normal fixtures.
Upload their `--0.png` files directly, without altering app content. Refresh
them when the app changes. Private browser captures and signed build
artifacts do not belong in this directory.

The store description must call sharing a test-data-only development preview
until the owning sharing/deployment contract changes. It must not imply that
files contain entries created after their saved point or promise clinical
advice. Store declarations and public availability are tracked in
[015](../../../docs/tactical/015-android-public-release.md).
