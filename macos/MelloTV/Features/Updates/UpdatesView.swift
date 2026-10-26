import SwiftUI

struct UpdatesView: View {
    let version: String
    let mode: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            DashboardSectionHeading(title: "Updates", detail: "Mello TV and its bundled providers.")
            VStack(spacing: 18) {
                updateRow("Installed version", value: "Mello TV \(version)")
                Divider()
                updateRow("Provider delivery", value: mode == "bundled" ? "Included with Mello TV" : "Status unavailable")
                Divider()
                updateRow("Update source", value: "Not configured")
            }
            .padding(22)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(MelloTVTheme.quietFill, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "info.circle").accessibilityHidden(true)
                Text("Provider updates currently ship with the app. A release source hasn't been configured for availability checks.")
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)
            }
            .font(.callout)
            .foregroundStyle(.secondary)
        }
        .frame(maxWidth: 640, alignment: .leading)
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    private func updateRow(_ title: String, value: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 20) {
            Text(title)
                .foregroundStyle(.secondary)
                .frame(width: 120, alignment: .leading)
            Text(value)
                .fontWeight(.medium)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
                .frame(minWidth: 0, maxWidth: .infinity, alignment: .leading)
        }
        .font(.body)
    }
}
