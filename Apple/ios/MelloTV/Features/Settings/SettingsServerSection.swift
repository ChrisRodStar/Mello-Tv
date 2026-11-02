import MelloKit
import SwiftUI

struct SettingsServerSection: View {
    let name: String
    let address: String

    var body: some View {
        SettingsGroup(title: "Server") {
            HStack(alignment: .top, spacing: 14) {
                Image(systemName: "server.rack")
                    .font(.title3)
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .frame(width: 24, height: 28)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 8) {
                    Text(name)
                        .font(.body.weight(.medium))
                        .foregroundStyle(MelloTVTheme.text)
                    Text(address)
                        .font(.footnote)
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .textSelection(.enabled)
                }
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(18)
        }
    }
}
