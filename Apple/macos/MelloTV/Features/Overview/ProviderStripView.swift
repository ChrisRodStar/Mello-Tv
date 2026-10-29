import SwiftUI
import MelloKit

struct ProviderStripView: View {
    let providers: [ServerSnapshot.ProviderStatus]
    let isReachable: Bool
    let onManage: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 15) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text("Your sources").font(.system(size: 13, weight: .semibold))
                Text(isReachable ? "\(providers.count) loaded" : "Unavailable")
                    .font(.system(size: 10)).foregroundStyle(.secondary)
                Spacer()
                Button("Manage", action: onManage)
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(.secondary)
                    .buttonStyle(.plain)
            }
            if providers.isEmpty {
                Text("Source details appear when the server is available.")
                    .font(.system(size: 11)).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 47, alignment: .leading)
            } else {
                HStack(spacing: 4) {
                    ForEach(providers) { provider in
                        SourceWordmark(provider: provider, isReachable: isReachable, onSelect: onManage)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
        }
    }
}

private struct SourceWordmark: View {
    let provider: ServerSnapshot.ProviderStatus
    let isReachable: Bool
    let onSelect: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: onSelect) {
            VStack(alignment: .leading, spacing: 8) {
                Text(provider.name)
                    .font(.system(size: 13, weight: .semibold)).tracking(-0.4)
                    .foregroundStyle(isReachable ? MelloTVTheme.text : MelloTVTheme.textSecondary)
                    .lineLimit(1).minimumScaleFactor(0.85)
                HStack(spacing: 5) {
                    Circle().fill(tone).frame(width: 4, height: 4).accessibilityHidden(true)
                    Text(status).font(.system(size: 9, weight: .medium)).foregroundStyle(MelloTVTheme.textSecondary)
                        .lineLimit(1)
                }
            }
            .padding(.vertical, 7)
            .padding(.horizontal, 6)
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
            .background(hovered ? MelloTVTheme.quietFill : .clear, in: RoundedRectangle(cornerRadius: 8))
        }
        .buttonStyle(OverviewPressStyle())
        .onHover { hovered = $0 }
        .help(isReachable ? provider.activity : "Server unavailable")
        .accessibilityLabel("\(provider.name), \(status). View provider details.")
    }

    private var status: String {
        if !isReachable { return "Offline" }
        if provider.needsAttention { return "Needs care" }
        if provider.lastFailure != nil { return "Past failure" }
        guard provider.observedAt != nil else { return "No activity" }
        return (provider.candidateCount ?? 0) > 0 ? "Responded" : "No matches"
    }

    private var tone: Color {
        if !isReachable || provider.observedAt == nil { return MelloTVTheme.mutedSubtle }
        if provider.needsAttention { return .orange }
        return provider.lastFailure == nil ? MelloTVTheme.accent.opacity(0.8) : MelloTVTheme.mutedSubtle
    }
}

typealias OverviewProviderStrip = ProviderStripView

