import SwiftUI

struct ProvidersView: View {
    let providers: [ServerSnapshot.ProviderStatus]
    let online: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            DashboardSectionHeading(title: "Providers", detail: online ? "Activity from the most recent title request." : "Showing the last received information.")
            if providers.isEmpty {
                DashboardEmptyState(symbol: "square.stack.3d.up", title: online ? "No providers loaded" : "Waiting for your server", detail: "Provider information appears when the server is available.")
            } else {
                Table(providers) {
                    TableColumn("Provider") { provider in
                        Text(provider.name).fontWeight(.medium)
                    }
                    .width(min: 80, ideal: 120)
                    TableColumn("Version", value: \.version).width(min: 55, ideal: 65, max: 90)
                    TableColumn("Last request") { provider in
                        Label(provider.activity, systemImage: provider.needsAttention ? "exclamationmark.circle" : provider.observedAt == nil ? "circle.dotted" : "checkmark.circle")
                            .foregroundStyle(provider.needsAttention ? Color.orange : .secondary)
                            .lineLimit(1)
                            .help(provider.activity)
                    }
                    TableColumn("When") { provider in
                        if let date = provider.observedAt {
                            Text(date, style: .relative).foregroundStyle(.secondary)
                        } else {
                            Text("—").foregroundStyle(.secondary)
                        }
                    }
                    .width(min: 65, ideal: 90, max: 120)
                }
                .tableStyle(.inset(alternatesRowBackgrounds: true))
            }
        }
        .padding(22)
    }
}

