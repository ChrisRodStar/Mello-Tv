import SwiftUI
import MelloKit

/// 10-foot Settings view displaying connection status and app info.
public struct SettingsView: View {
    @ObservedObject var serverAddress = ServerAddress.shared
    @State private var serverStatus: ServerSnapshot?
    @State private var isLoading = false

    public init() {}

    public var body: some View {
        ScrollView(.vertical, showsIndicators: false) {
            VStack(alignment: .leading, spacing: 48) {
                Text("Settings")
                    .font(.system(size: 40, weight: .bold))
                    .foregroundColor(.white)

                // Server Section
                VStack(alignment: .leading, spacing: 20) {
                    Text("Server Connection")
                        .font(.title2.weight(.bold))
                        .foregroundColor(.white)

                    HStack(spacing: 32) {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("Active Server")
                                .font(.caption.weight(.bold))
                                .foregroundColor(.secondary)
                            Text(serverAddress.serverName)
                                .font(.title3.weight(.semibold))
                                .foregroundColor(.primary)
                            Text(serverAddress.activeBaseURL?.absoluteString ?? "Not connected")
                                .font(.callout)
                                .foregroundColor(.secondary)
                        }

                        Spacer()

                        Button("Disconnect", role: .destructive) {
                            serverAddress.disconnect()
                        }
                        .buttonStyle(.bordered)
                    }
                    .padding(28)
                    .background(Color(white: 0.15))
                    .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
                }

                // App Info
                VStack(alignment: .leading, spacing: 20) {
                    Text("About Mello TV for Apple TV")
                        .font(.title2.weight(.bold))
                        .foregroundColor(.white)

                    VStack(alignment: .leading, spacing: 14) {
                        HStack {
                            Text("Version")
                                .foregroundColor(.secondary)
                            Spacer()
                            Text("0.3.0 (Build 5)")
                                .fontWeight(.medium)
                        }

                        Divider()

                        HStack {
                            Text("Platform")
                                .foregroundColor(.secondary)
                            Spacer()
                            Text("Apple TV (tvOS 17+)")
                                .fontWeight(.medium)
                        }

                        Divider()

                        HStack {
                            Text("Architecture")
                                .foregroundColor(.secondary)
                            Spacer()
                            Text("Native Shell Client")
                                .fontWeight(.medium)
                        }
                    }
                    .padding(28)
                    .background(Color(white: 0.15))
                    .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
                }
            }
            .padding(.horizontal, 64)
            .padding(.top, 32)
            .padding(.bottom, 64)
        }
    }
}
