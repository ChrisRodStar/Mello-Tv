import AppKit
import SwiftUI

struct NetworkView: View {
    let port: Int
    let endpoints: [String]

    private var connections: [DashboardConnection] {
        var seen = Set<String>()
        return ([AppLinks.clientURL(port: port).absoluteString] + endpoints).compactMap { raw in
            guard let url = URL(string: raw), let host = url.host,
                  ["http", "https"].contains(url.scheme ?? ""), url.user == nil, url.password == nil else { return nil }
            let address = raw.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
            guard seen.insert(address).inserted else { return nil }
            let octets = host.split(separator: ".").compactMap { Int($0) }
            let roaming = octets.count == 4 && octets[0] == 100 && (64...127).contains(octets[1])
            return DashboardConnection(address: address, kind: host == "127.0.0.1" ? "This Mac" : roaming ? "Tailscale" : "Home network")
        }
    }

    var body: some View {
        DesktopPage(maxWidth: 780) {
            VStack(alignment: .leading, spacing: 28) {
                DesktopPageHeading(title: "Network", detail: "Connect from the iOS app or a browser.")
                VStack(spacing: 14) {
                    ForEach(["This Mac", "Home network", "Tailscale"], id: \.self) { kind in
                        let addresses = connections.filter { $0.kind == kind }
                        if let primary = addresses.first {
                            connectionCard(primary, alternatives: Array(addresses.dropFirst()))
                        }
                    }
                }
                Text("The iOS app finds your server automatically on the same Wi-Fi. Use Tailscale to connect away from home.")
                    .font(.system(size: 12)).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func connectionCard(_ connection: DashboardConnection, alternatives: [DashboardConnection]) -> some View {
        HStack(spacing: 18) {
            Image(systemName: connection.kind == "This Mac" ? "desktopcomputer" : connection.kind == "Tailscale" ? "network" : "wifi")
                .font(.system(size: 22, weight: .medium))
                .foregroundStyle(MelloTVTheme.accent)
                .frame(width: 48, height: 48)
                .background(MelloTVTheme.accent.opacity(0.10), in: Circle())
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 7) {
                Text(connection.kind).font(.system(size: 16, weight: .semibold))
                Text(connection.address).font(.system(size: 12, design: .monospaced))
                    .foregroundStyle(.secondary).textSelection(.enabled)
                    .lineLimit(1).truncationMode(.middle)
            }
            Spacer(minLength: 8)
            if !alternatives.isEmpty {
                Menu {
                    ForEach(alternatives) { address in
                        Button("Copy \(address.address)") { copy(address.address) }
                    }
                } label: {
                    Text("\(alternatives.count) more")
                        .font(.system(size: 11)).foregroundStyle(.secondary)
                }
                .fixedSize()
                .menuStyle(.borderlessButton)
                .help("Additional addresses")
            }
            Button { copy(connection.address) } label: {
                Image(systemName: "doc.on.doc")
            }
            .buttonStyle(StudioButtonStyle())
            .help("Copy \(connection.kind) address")
            .accessibilityLabel("Copy \(connection.kind) address")
        }
        .padding(22)
        .modifier(StudioSurface())
    }

    private func copy(_ address: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(address, forType: .string)
    }

}

private struct DashboardConnection: Identifiable {
    var id: String { address }
    let address: String
    let kind: String
}

