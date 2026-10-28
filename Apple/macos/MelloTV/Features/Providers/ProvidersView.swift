import SwiftUI

struct ProvidersView: View {
    let providers: [ServerSnapshot.ProviderStatus]
    let online: Bool

    var body: some View {
        DesktopPage {
            VStack(alignment: .leading, spacing: 28) {
                DesktopPageHeading(title: "Providers", detail: online ? "Activity from your latest title requests." : "Last received activity. Your server is unavailable.")
                if providers.isEmpty {
                    DashboardEmptyState(symbol: "square.stack.3d.up", title: "Waiting for providers", detail: "Provider information appears when your server is available.")
                        .frame(height: 300)
                } else {
                    LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 14) {
                        ForEach(providers) { provider in
                            ProviderCard(provider: provider, online: online)
                        }
                    }
                }
            }
        }
    }
}

private struct ProviderCard: View {
    let provider: ServerSnapshot.ProviderStatus
    let online: Bool

    private var color: Color {
        !online || provider.observedAt == nil ? .secondary : provider.needsAttention ? .orange : MelloTVTheme.accent
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 19) {
            HStack(spacing: 12) {
                Text(String(provider.name.prefix(1)))
                    .font(.system(size: 18, weight: .bold))
                    .foregroundStyle(MelloTVTheme.accent)
                    .frame(width: 38, height: 38)
                    .background(MelloTVTheme.accent.opacity(0.10), in: RoundedRectangle(cornerRadius: 12))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 4) {
                    Text(provider.name).font(.system(size: 16, weight: .semibold))
                    Text(provider.version).font(.system(size: 11)).foregroundStyle(.secondary)
                }
                Spacer(minLength: 0)
                Circle().fill(color).frame(width: 6, height: 6).accessibilityHidden(true)
            }
            HStack(spacing: 8) {
                Text(online ? provider.activity : "Server unavailable")
                    .font(.system(size: 12)).foregroundStyle(.secondary).lineLimit(1)
                    .help(provider.activity)
                Spacer(minLength: 0)
                if let date = provider.observedAt {
                    Text(date, style: .relative)
                        .font(.system(size: 10)).foregroundStyle(.tertiary).lineLimit(1)
                }
            }
        }
        .padding(20)
        .modifier(StudioSurface())
    }
}
