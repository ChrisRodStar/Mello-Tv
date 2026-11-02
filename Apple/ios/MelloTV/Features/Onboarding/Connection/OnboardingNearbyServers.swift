import MelloKit
import SwiftUI

struct OnboardingNearbyServers: View {
    let profileStore: ProfileStore
    let isConnected: Bool
    let isBusy: Bool
    var onSearch: () -> Void
    var onSelect: (DiscoveredServer) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text(isConnected ? "Your server" : "Nearby servers").font(
                    .subheadline.weight(.semibold))
                Spacer()
                Button(action: onSearch) {
                    Text(profileStore.isDiscoveringServer ? "Searching…" : "Search again")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .touchTarget()
                }
                .disabled(isBusy)
                .buttonStyle(.plain)
            }
            if isConnected {
                serverRow(
                    name: profileStore.connectedServerName,
                    address: profileStore.connectedServerAddress, connected: true)
            } else {
                ForEach(profileStore.discoveredServers) { server in
                    Button {
                        onSelect(server)
                    } label: {
                        serverRow(
                            name: server.name, address: server.primaryURL.absoluteString,
                            connected: false)
                    }
                    .buttonStyle(CardScaleButtonStyle())
                    .disabled(isBusy)
                }
                if profileStore.discoveredServers.isEmpty {
                    HStack(alignment: .top, spacing: 14) {
                        if isBusy {
                            ProgressView().tint(MelloTVTheme.text)
                        } else {
                            PhosphorIcon("wifi-slash", size: 22).accessibilityHidden(true)
                        }
                        VStack(alignment: .leading, spacing: 5) {
                            Text(isBusy ? "Looking for your server" : "No server found yet")
                                .font(.subheadline.weight(.semibold))
                            Text(
                                isBusy
                                    ? "This may take a few moments."
                                    : "Check that your server is running and you're on the same Wi-Fi or connected via Tailscale."
                            )
                            .font(.subheadline)
                            .foregroundStyle(MelloTVTheme.textSecondary)
                        }
                        .fixedSize(horizontal: false, vertical: true)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(20)
                    .appleNeumorphicRecessed(cornerRadius: 16)
                }
            }
        }
    }

    // MARK: - Server Row

    private func serverRow(name: String, address: String, connected: Bool) -> some View {
        HStack(spacing: 14) {
            PhosphorIcon("television", size: 26).accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 5) {
                Text(name).font(.subheadline.weight(.semibold))
                Text(address).font(.caption).foregroundStyle(MelloTVTheme.textSecondary)
                    .lineLimit(2).truncationMode(.middle)
                if connected {
                    Text("Connected").font(.caption.weight(.medium)).foregroundStyle(
                        MelloTVTheme.accent)
                }
            }
            Spacer(minLength: 0)
            PhosphorIcon(connected ? "check-circle" : "caret-right", size: 22)
                .foregroundStyle(connected ? MelloTVTheme.accent : MelloTVTheme.textSecondary)
                .accessibilityHidden(true)
        }
        .foregroundStyle(MelloTVTheme.text)
        .multilineTextAlignment(.leading)
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .appleNeumorphic(cornerRadius: 16, elevation: .subtle)
        .accessibilityElement(children: .combine)
    }

}
