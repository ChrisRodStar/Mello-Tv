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

    var body: some View {
        GeometryReader { viewport in
            let compact = viewport.size.height < 600
            let inset: CGFloat = compact ? 24 : 30
            VStack(alignment: .leading, spacing: compact ? 20 : 26) {
                pageHeader

                HStack(spacing: compact ? 28 : 36) {
                    cinemaStage(compact: compact)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .layoutPriority(1)
                    serverDetails(compact: compact)
                        .frame(width: min(310, viewport.size.width * 0.36))
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)

                OverviewProviderStrip(providers: providers, isReachable: online) {
                    destination = .providers
                }
                .padding(.top, compact ? 18 : 22)
                .overlay(alignment: .top) {
                    Rectangle().fill(MelloTVTheme.border.opacity(0.65)).frame(height: 1)
                }
            }
            .padding(inset)
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
    }

    private func cinemaStage(compact: Bool) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 7) {
                Circle()
                    .fill(online ? MelloTVTheme.accent : process.isRunning ? .orange : MelloTVTheme.mutedSubtle)
                    .frame(width: 6, height: 6)
                    .accessibilityHidden(true)
                Text(online ? "Running on this Mac" : process.isRunning ? "Connecting to this Mac" : "Server stopped")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(MelloTVTheme.textSecondary)
            }
            .padding(.bottom, compact ? 14 : 20)

            Text(headline)
                .font(.system(size: compact ? 34 : 42, weight: .semibold, design: .rounded))
                .tracking(-1.4)
                .lineSpacing(-1)
                .foregroundStyle(MelloTVTheme.text)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityAddTraits(.isHeader)

            ServerPortraitView(isOnline: online, needsAttention: !ready && online)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .padding(.horizontal, -18)
                .padding(.bottom, -12)
        }
        .padding(compact ? 26 : 32)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background {
            RoundedRectangle(cornerRadius: 34, style: .continuous)
                .fill(LinearGradient(colors: [MelloTVTheme.card, MelloTVTheme.sunken],
                                     startPoint: .topLeading, endPoint: .bottomTrailing))
                .overlay {
                    RoundedRectangle(cornerRadius: 34, style: .continuous)
                        .strokeBorder(online ? MelloTVTheme.accent.opacity(0.22) : MelloTVTheme.subtleBorder,
                                      lineWidth: 1)
                }
                .shadow(color: .black.opacity(0.12), radius: 16, x: 0, y: 12)
        }
    }

    private func serverDetails(compact: Bool) -> some View {
        VStack(alignment: .leading, spacing: compact ? 18 : 24) {
            VStack(alignment: .leading, spacing: 10) {
                Text(ready ? "At home,\nalways ready." : "Your Mac.\nYour cinema.")
                    .font(.system(size: compact ? 25 : 29, weight: .medium, design: .rounded))
                    .tracking(-0.8)
                    .foregroundStyle(MelloTVTheme.text)
                    .fixedSize(horizontal: false, vertical: true)
                Text(detail)
                    .font(.system(size: 12))
                    .lineSpacing(4)
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            VStack(spacing: 0) {
                metric(title: "Uptime", value: online ? uptime : "—", symbol: "clock")
                Divider().overlay(MelloTVTheme.border.opacity(0.4))
                metric(title: "Library", value: !online ? "Offline" : libraryReady ? "Available" : "Needs care", symbol: "rectangle.stack")
                Divider().overlay(MelloTVTheme.border.opacity(0.4))
                Button { destination = .network } label: {
                    metric(title: "Connection", value: connection, symbol: "network")
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help("View network addresses")
            }

            serverControls
        }
        .padding(.vertical, compact ? 8 : 18)
        .frame(maxHeight: .infinity, alignment: .center)
    }

    private var serverControls: some View {
        HStack(spacing: 10) {
            Button(action: actions.toggleServer) {
                HStack(spacing: 8) {
                    Image(systemName: "power")
                    Text(process.isRunning ? "Stop server" : "Start server")
                }
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(MelloTVTheme.text)
                .padding(.horizontal, 18)
                .frame(height: 44)
                .background {
                    RoundedRectangle(cornerRadius: 16, style: .continuous)
                        .fill(LinearGradient(colors: [MelloTVTheme.cardHover, MelloTVTheme.card],
                                             startPoint: .topLeading, endPoint: .bottomTrailing))
                        .overlay {
                            RoundedRectangle(cornerRadius: 16, style: .continuous)
                                .strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 1)
                        }
                        .shadow(color: .black.opacity(0.14), radius: 8, x: 3, y: 6)
                }
            }
            .buttonStyle(OverviewPressStyle())

            if process.isRunning {
                Button(action: actions.restartServer) {
                    Image(systemName: "arrow.clockwise")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .frame(width: 44, height: 44)
                        .background(MelloTVTheme.sunken, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                }
                .buttonStyle(OverviewPressStyle())
                .help("Restart server")
                .accessibilityLabel("Restart server")
            }
        }
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

    private func metric(title: String, value: String, symbol: String) -> some View {
        HStack(spacing: 13) {
            Image(systemName: symbol)
                .font(.system(size: 14, weight: .regular))
                .foregroundStyle(MelloTVTheme.textSecondary)
                .frame(width: 38, height: 38)
                .background(MelloTVTheme.sunken.opacity(0.65), in: RoundedRectangle(cornerRadius: 13, style: .continuous))
            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle(MelloTVTheme.textSecondary)
                Text(value)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(MelloTVTheme.text)
                    .monospacedDigit()
                    .tracking(-0.2)
                    .lineLimit(1)
                    .minimumScaleFactor(0.85)
            }
            Spacer(minLength: 0)
            if symbol == "network" {
                Image(systemName: "arrow.up.right")
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle(MelloTVTheme.textSecondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.vertical, 12)
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
