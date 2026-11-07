import SwiftUI
import MelloKit

/// First-run server pairing view with Bonjour mDNS local detection and manual IP entry.
public struct ServerDiscoveryView: View {
    @ObservedObject var serverAddress = ServerAddress.shared
    @State private var manualHost = "192.168.1."
    @State private var manualPort = "8080"
    @State private var isConnecting = false
    @State private var connectionFailed = false

    public init() {}

    public var body: some View {
        VStack(spacing: 36) {
            // Header
            VStack(spacing: 12) {
                Image(systemName: "appletv.fill")
                    .font(.system(size: 72))
                    .foregroundColor(.white)

                Text("Connect to Mello TV Server")
                    .font(.system(size: 40, weight: .bold, design: .rounded))
                    .foregroundColor(.white)

                Text("Make sure your Mello TV Mac server or Docker instance is running on this network.")
                    .font(.body)
                    .foregroundColor(.secondary)
            }
            .padding(.top, 40)

            // Discovered Servers Shelf
            VStack(alignment: .leading, spacing: 18) {
                HStack(spacing: 12) {
                    Text("Discovered Servers")
                        .font(.title3.weight(.bold))
                        .foregroundColor(.white)

                    if serverAddress.isScanning {
                        ProgressView()
                            .scaleEffect(0.8)
                    }
                }

                if serverAddress.discoveredServers.isEmpty {
                    Text("Searching your home network via Bonjour…")
                        .font(.callout)
                        .foregroundColor(.secondary)
                        .padding(.vertical, 16)
                } else {
                    HStack(spacing: 24) {
                        ForEach(serverAddress.discoveredServers) { server in
                            Button {
                                Task {
                                    await connect(to: server.url, name: server.name)
                                }
                            } label: {
                                VStack(alignment: .leading, spacing: 6) {
                                    HStack {
                                        Image(systemName: "desktopcomputer")
                                        Text(server.name)
                                            .fontWeight(.bold)
                                    }
                                    Text(server.url.host ?? "")
                                        .font(.caption)
                                        .foregroundColor(.secondary)
                                }
                                .padding(.horizontal, 28)
                                .padding(.vertical, 20)
                            }
                            .buttonStyle(.borderedProminent)
                        }
                    }
                }
            }
            .frame(maxWidth: 900, alignment: .leading)

            // Manual Connection Fallback
            VStack(alignment: .leading, spacing: 16) {
                Text("Manual Connection")
                    .font(.title3.weight(.bold))
                    .foregroundColor(.white)

                HStack(spacing: 24) {
                    TextField("Server IP (e.g. 192.168.1.100)", text: $manualHost)
                        .frame(width: 440)

                    TextField("Port", text: $manualPort)
                        .frame(width: 180)

                    Button {
                        let urlString = "http://\(manualHost.trimmingCharacters(in: .whitespaces)):\(manualPort.trimmingCharacters(in: .whitespaces))"
                        if let url = URL(string: urlString) {
                            Task {
                                await connect(to: url, name: "Custom Server")
                            }
                        }
                    } label: {
                        Text("Connect")
                            .fontWeight(.semibold)
                            .frame(minWidth: 140)
                    }
                    .buttonStyle(.borderedProminent)
                }

                if connectionFailed {
                    Text("Could not reach Mello TV server at this address. Check port and Wi-Fi.")
                        .font(.caption)
                        .foregroundColor(.red)
                }
            }
            .frame(maxWidth: 900, alignment: .leading)
            .padding(.top, 16)

            Spacer()
        }
        .padding(.horizontal, 64)
    }

    private func connect(to url: URL, name: String) async {
        isConnecting = true
        connectionFailed = false
        let success = await serverAddress.connect(to: url, name: name)
        isConnecting = false
        connectionFailed = !success
    }
}
