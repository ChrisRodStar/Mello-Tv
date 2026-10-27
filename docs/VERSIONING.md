# Versioning

The current product release is **0.1.0-alpha.2**, a pre-release under [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html).

## Release version and Apple bundle numbers

The complete release string is kept separately from Apple’s numeric bundle fields:

| Purpose | Current value | Setting |
| --- | --- | --- |
| Product release | `0.1.0-alpha.2` | Rust workspace/package versions, web package, release labels, and `MelloTVReleaseVersion` |
| Apple marketing version | `0.1.0` | `CFBundleShortVersionString` / Xcode `MARKETING_VERSION` |
| Apple build number | `2` | `CFBundleVersion` / Xcode `CURRENT_PROJECT_VERSION` |

This is intentional. Apple requires `CFBundleShortVersionString` to contain three period-separated numeric components, and `CFBundleVersion` to use numeric components. The SemVer pre-release suffix therefore remains in `MelloTVReleaseVersion`; it is not omitted from the product release identity.

References: [Apple `CFBundleShortVersionString`](https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundleshortversionstring), [Apple `CFBundleVersion`](https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundleversion), and [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html).

## Updating a pre-release

For the next alpha, advance the full release string to `0.1.0-alpha.3`, keep the Apple marketing version at `0.1.0`, and increment the Apple build number to `3`. Increment the build number for each subsequent alpha build (`alpha.4` uses build `4`, and so on). When the numeric product version changes, update the marketing version to its numeric `MAJOR.MINOR.PATCH` core and continue increasing the Apple build number.
