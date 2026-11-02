import MelloKit
import SwiftUI
import UIKit

struct AboutAppHeader: View {
    private var version: String {
        let release = Bundle.main.object(forInfoDictionaryKey: "MelloTVReleaseVersion") as? String
        if let release, !release.isEmpty { return release }
        return Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
            ?? "—"
    }

    /// Load the app's compiled icon rather than maintaining a separate illustration.
    private var appIcon: UIImage? {
        let icons = Bundle.main.infoDictionary?["CFBundleIcons"] as? [String: Any]
        let primaryIcon = icons?["CFBundlePrimaryIcon"] as? [String: Any]
        let filenames = primaryIcon?["CFBundleIconFiles"] as? [String]
        return filenames?.reversed().compactMap { UIImage(named: $0) }.first
    }

    var body: some View {
        VStack(spacing: 20) {
            Group {
                if let appIcon {
                    Image(uiImage: appIcon)
                        .resizable()
                        .interpolation(.high)
                        .scaledToFit()
                } else {
                    PhosphorIcon("brand-tv-mark", size: 52)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .background(MelloTVTheme.cardBackground)
                }
            }
            .frame(width: 96, height: 96)
            .clipShape(.rect(cornerRadius: 22))
            .shadow(color: .black.opacity(0.14), radius: 16, x: 0, y: 8)
            .accessibilityHidden(true)

            VStack(spacing: 8) {
                Text("Mello TV")
                    .font(.largeTitle.weight(.bold))
                    .tracking(-1)
                    .foregroundStyle(MelloTVTheme.text)
                    .accessibilityAddTraits(.isHeader)
                Text("Version " + version)
                    .font(.subheadline.monospacedDigit())
                    .foregroundStyle(MelloTVTheme.textSecondary)
            }
        }
        .multilineTextAlignment(.center)
        .frame(maxWidth: .infinity)
        .padding(.top, 12)
        .padding(.bottom, 8)
    }
}
