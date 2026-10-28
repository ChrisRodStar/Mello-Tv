# Versioning

The product uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html),
including pre-release labels such as `0.1.0-alpha.3`.

The release identity is stored in the root `Cargo.toml`, `web/package.json`, and
Apple `MelloTVReleaseVersion` settings. Keep these aligned during a release.

Apple bundle fields remain numeric:

- `CFBundleShortVersionString` / `MARKETING_VERSION`: the numeric
  `MAJOR.MINOR.PATCH` core, without a pre-release suffix.
- `CFBundleVersion` / `CURRENT_PROJECT_VERSION`: an increasing build number.

Advance the full release label for each release and increase the Apple build
number. Update the marketing version when the numeric product version changes.
See [macOS packaging](../Apple/macos/README.md#build-and-install) for the local
release workflow.

References: [Apple marketing version](https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundleshortversionstring)
and [Apple build number](https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundleversion).
