import SwiftUI
import MelloKit

struct OverviewView: View {
    @ObservedObject var model: ServerStatusStore
    @ObservedObject var process: ServerProcessState
    let actions: AppActions
    @Binding var destination: DesktopDestination

    private var online: Bool { process.isRunning && model.isReachable }
    private var providers: [ServerSnapshot.ProviderStatus] { model.snapshot?.media.providers ?? [] }
    private var libraryReady: Bool { online && model.snapshot?.catalogStatus == "ready" && model.snapshot?.catalogSnapshot == "retained" }
    private var mediaReady: Bool { online && model.snapshot?.media.configured == true }
    private var attention: Int { providers.filter(\.needsAttention).count }
    private var ready: Bool { online && libraryReady && mediaReady }

    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        GeometryReader { viewport in
            let compact = viewport.size.height < 600
            VStack(alignment: .leading, spacing: compact ? 22 : 28) {
                pageHeader
                serverPanel(compact: compact)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                OverviewProviderStrip(providers: providers, isReachable: online) {
                    destination = .providers
                }
                .padding(.horizontal, 4)
                .padding(.bottom, 4)
            }
            .padding(compact ? 26 : 34)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
    }

    private var pageHeader: some View {
        HStack(alignment: .firstTextBaseline) {
            Text("Overview")
                .font(.system(size: 20, weight: .semibold))
                .tracking(-0.4)
                .foregroundStyle(MelloTVTheme.text)
            Spacer()
            if let updated = model.lastRefreshed {
                Text("Updated \(updated.formatted(date: .omitted, time: .shortened))")
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle(MelloTVTheme.textSecondary)
            }
        }
        .padding(.horizontal, 4)
    }

    private func serverPanel(compact: Bool) -> some View {
        VStack(alignment: .leading, spacing: compact ? 24 : 34) {
            HStack(alignment: .center, spacing: 32) {
                VStack(alignment: .leading, spacing: compact ? 18 : 24) {
                    HStack(spacing: 8) {
                        Circle()
                            .fill(statusTint)
                            .frame(width: 6, height: 6)
                            .shadow(color: statusTint.opacity(online ? 0.3 : 0), radius: 4)
                            .accessibilityHidden(true)
                        Text(online ? "Running on this Mac" : process.isRunning ? "Connecting to this Mac" : "Server stopped")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundStyle(MelloTVTheme.textSecondary)
                    }

                    VStack(alignment: .leading, spacing: 14) {
                        Text(headline)
                            .font(.system(size: compact ? 36 : 44, weight: .semibold))
                            .tracking(-1.6)
                            .lineSpacing(-1)
                            .foregroundStyle(MelloTVTheme.text)
                            .fixedSize(horizontal: false, vertical: true)
                            .accessibilityAddTraits(.isHeader)
                        Text(detail)
                            .font(.system(size: 13))
                            .lineSpacing(4)
                            .foregroundStyle(MelloTVTheme.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                serverControls
                    .frame(width: compact ? 120 : 140)
            }
            .padding(.horizontal, compact ? 4 : 8)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)

            systemRail
        }
        .padding(compact ? 26 : 34)
        .background {
            RoundedRectangle(cornerRadius: 30, style: .continuous)
                .fill(LinearGradient(
                    stops: [
                        .init(color: MelloTVTheme.surfaceHighlight, location: 0),
                        .init(color: MelloTVTheme.card, location: 0.48),
                        .init(color: MelloTVTheme.surfaceShade, location: 1)
                    ],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                ))
                .overlay {
                    RoundedRectangle(cornerRadius: 30, style: .continuous)
                        .strokeBorder(LinearGradient(
                            colors: [.white.opacity(colorScheme == .dark ? 0.10 : 0.85), MelloTVTheme.subtleBorder],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        ), lineWidth: 1)
                }
                .shadow(color: MelloTVTheme.ambientShadow.opacity(colorScheme == .dark ? 0.25 : 0.10),
                        radius: 18, x: 0, y: 12)
        }
    }

    private var serverControls: some View {
        VStack(spacing: 16) {
            Button(action: actions.toggleServer) {
                VStack(spacing: 13) {
                    Image(systemName: "power")
                        .font(.system(size: 28, weight: .light))
                        .foregroundStyle(process.isRunning ? statusTint : MelloTVTheme.textSecondary)
                        .frame(width: 88, height: 88)
                        .background {
                            Circle()
                                .fill(LinearGradient(colors: [MelloTVTheme.surfaceHighlight, MelloTVTheme.surfaceShade],
                                                     startPoint: .topLeading, endPoint: .bottomTrailing))
                                .overlay {
                                    Circle().strokeBorder(process.isRunning ? statusTint.opacity(0.25) : MelloTVTheme.subtleBorder,
                                                          lineWidth: 1)
                                }
                                .shadow(color: .white.opacity(colorScheme == .dark ? 0.04 : 0.65), radius: 4, x: -3, y: -3)
                                .shadow(color: MelloTVTheme.ambientShadow.opacity(colorScheme == .dark ? 0.4 : 0.18),
                                        radius: 10, x: 4, y: 7)
                        }
                    Text(process.isRunning ? "Stop server" : "Start server")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(MelloTVTheme.text)
                }
                .padding(6)
                .contentShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
            }
            .buttonStyle(OverviewPressStyle())
            .help(process.isRunning ? "Stop the local server" : "Start the local server")
            .accessibilityLabel(process.isRunning ? "Stop server" : "Start server")

            if process.isRunning {
                Button(action: actions.restartServer) {
                    Label("Restart", systemImage: "arrow.clockwise")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                        .contentShape(Capsule())
                }
                .buttonStyle(OverviewPressStyle())
                .help("Restart server")
                .accessibilityLabel("Restart server")
            }
        }
    }

    private var systemRail: some View {
        HStack(alignment: .center, spacing: 18) {
            metric(title: "Uptime", value: online ? uptime : "—")
            railDivider
            metric(title: "Library", value: !online ? "Offline" : libraryReady ? "Available" : "Needs care")
            railDivider
            Button { destination = .network } label: {
                HStack(spacing: 8) {
                    metric(title: "Connection", value: connection)
                    Image(systemName: "arrow.up.right")
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(OverviewPressStyle())
            .help("View network addresses")
        }
        .padding(.horizontal, 22)
        .padding(.vertical, 20)
        .background {
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .fill(MelloTVTheme.sunken.opacity(0.65))
                .overlay {
                    RoundedRectangle(cornerRadius: 20, style: .continuous)
                        .strokeBorder(LinearGradient(
                            colors: [.black.opacity(colorScheme == .dark ? 0.22 : 0.08), .white.opacity(colorScheme == .dark ? 0.04 : 0.65)],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        ), lineWidth: 1)
                }
        }
    }

    private var railDivider: some View {
        Rectangle()
            .fill(MelloTVTheme.border.opacity(0.65))
            .frame(width: 1, height: 32)
            .accessibilityHidden(true)
    }

    private var statusTint: Color {
        if !process.isRunning { return MelloTVTheme.mutedSubtle }
        if !online || !ready || attention > 0 { return .orange }
        return MelloTVTheme.accent
    }

    private var headline: String {
        if !process.isRunning { return "Your cinema,\non pause." }
        if !online { return model.error == nil ? "Waking up\nyour cinema." : "Let's reconnect\nyour cinema." }
        if !libraryReady || !mediaReady { return "Your server\nneeds attention." }
        return "Your cinema\nis online."
    }

    private var detail: String {
        if !process.isRunning { return "Start Mello TV to share your library." }
        if !online { return model.error ?? "The server is getting ready on this Mac." }
        if !mediaReady { return "The media engine needs attention. Open the logs for details." }
        if !libraryReady { return "The server is running. Your catalog needs attention." }
        if attention > 0 { return "\(attention) source\(attention == 1 ? " needs" : "s need") attention. Your server is available." }
        return "A quiet home for everything you love to watch."
    }

    private func metric(title: String, value: String) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            Text(title)
                .font(.system(size: 10, weight: .medium))
                .foregroundStyle(MelloTVTheme.textSecondary)
            Text(value)
                .font(.system(size: 16, weight: .medium))
                .foregroundStyle(MelloTVTheme.text)
                .monospacedDigit()
                .tracking(-0.3)
                .lineLimit(1)
                .minimumScaleFactor(0.8)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var uptime: String {
        guard let seconds = model.snapshot?.uptimeSeconds else { return "—" }
        if seconds < 60 { return "\(seconds)s" }
        if seconds < 3600 { return "\(seconds / 60)m \(seconds % 60)s" }
        if seconds < 86400 { return "\(seconds / 3600)h \((seconds % 3600) / 60)m" }
        return "\(seconds / 86400)d \((seconds % 86400) / 3600)h"
    }

    private var connection: String {
        guard online else { return "Offline" }
        let roaming = model.snapshot?.endpoints.contains { endpoint in
            guard let host = URL(string: endpoint)?.host else { return false }
            let octets = host.split(separator: ".").compactMap { Int($0) }
            return octets.count == 4 && octets[0] == 100 && (64...127).contains(octets[1])
        } ?? false
        return roaming ? "Tailscale + LAN" : "Local network"
    }
}

struct OverviewPressStyle: ButtonStyle {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .opacity(configuration.isPressed ? 0.78 : 1)
            .scaleEffect(configuration.isPressed && !reduceMotion ? 0.98 : 1)
            .animation(reduceMotion ? nil : .easeOut(duration: 0.15), value: configuration.isPressed)
    }
}
