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
        VStack(alignment: .leading, spacing: 16) {
            DashboardSectionHeading(title: "Network", detail: "The iOS app discovers your server on the same Wi-Fi.")
            Table(connections) {
                TableColumn("Connection", value: \.kind).width(min: 100, ideal: 130, max: 170)
                TableColumn("Address") { connection in
                    Text(connection.address).font(.system(.body, design: .monospaced))
                        .textSelection(.enabled).lineLimit(1).truncationMode(.middle)
                }
                TableColumn("") { connection in
                    Button {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(connection.address, forType: .string)
                    } label: { Image(systemName: "doc.on.doc") }
                        .buttonStyle(.borderless)
                        .help("Copy address")
                        .accessibilityLabel("Copy \(connection.kind) address")
                }
                .width(32)
            }
            .tableStyle(.inset(alternatesRowBackgrounds: true))
        }
        .padding(22)
    }
}

private struct DashboardConnection: Identifiable {
    var id: String { address }
    let address: String
    let kind: String
}

