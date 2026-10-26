import SwiftUI

struct OverviewView: View {
    @ObservedObject var model: ServerStatusStore
    @ObservedObject var process: ServerProcessState
    let actions: AppActions
    @Binding var destination: DesktopDestination
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private var online: Bool { process.isRunning && model.isReachable }
    private var providers: [ServerSnapshot.ProviderStatus] { model.snapshot?.media.providers ?? [] }
    private var libraryReady: Bool { online && model.snapshot?.catalogStatus == "ok" && model.snapshot?.catalogSnapshot == "retained" }
    private var mediaReady: Bool { online && model.snapshot?.media.configured == true }
    private var attention: Int { providers.filter(\.needsAttention).count }
    private var ready: Bool { online && libraryReady && mediaReady }

    var body: some View {
        GeometryReader { viewport in
            let short = viewport.size.height < 590
            let inset: CGFloat = short ? 24 : 32
            let showPortrait = viewport.size.width >= 620
            VStack(alignment: .leading, spacing: 0) {
                pageHeader
                HStack(spacing: 12) {
                    statusComposition(short: short)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    if showPortrait {
                        ServerPortraitView(isOnline: online, needsAttention: !ready && online)
                            .frame(width: min(240, viewport.size.width * 0.32))
                            .padding(.trailing, 4)
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .padding(.vertical, short ? 20 : 28)

                systemRail(short: short)
                OverviewProviderStrip(providers: providers, isReachable: online) {
                    destination = .providers
                }
                .padding(.top, short ? 20 : 28)
            }
            .padding(inset)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .animation(reduceMotion ? nil : .easeInOut(duration: 0.25), value: online)
        }
    }

    private var pageHeader: some View {
        HStack(alignment: .firstTextBaseline) {
            Text("Overview")
                .font(.system(size: 20, weight: .medium))
                .tracking(-0.4)
            Spacer()
            if let updated = model.lastRefreshed {
                HStack(spacing: 5) {
                    Circle().fill(.white.opacity(0.3)).frame(width: 3, height: 3)
                    Text("Updated \(updated.formatted(date: .omitted, time: .shortened))")
                }
                .font(.system(size: 10, weight: .medium))
                .foregroundStyle(.secondary)
            }
        }
    }

    private func statusComposition(short: Bool) -> some View {
        VStack(alignment: .leading, spacing: short ? 16 : 22) {
            HStack(spacing: 8) {
                Circle()
                    .fill(online ? Color(red: 0.43, green: 0.82, blue: 0.64) : process.isRunning ? .orange : .secondary)
                    .frame(width: 6, height: 6)
                    .accessibilityHidden(true)
                Text(online ? "Running on this Mac" : process.isRunning ? "Connecting to this Mac" : "Server stopped")
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(.secondary)
            }
            VStack(alignment: .leading, spacing: 12) {
                Text(headline)
                    .font(.system(size: short ? 32 : 38, weight: .semibold))
                    .tracking(short ? -1.1 : -1.5)
                    .lineSpacing(-1)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
                Text(detail)
                    .font(.system(size: 13))
                    .lineSpacing(4)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack(spacing: 14) {
                Button(action: actions.toggleServer) {
                    Label(process.isRunning ? "Stop server" : "Start server", systemImage: "power")
                        .font(.system(size: 12, weight: .medium))
                        .padding(.horizontal, 13)
                        .frame(height: 34)
                        .foregroundStyle(process.isRunning ? Color.white.opacity(0.88) : Color.white)
                        .background(process.isRunning ? Color.white.opacity(0.07) : MelloTVTheme.accent,
                                    in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                        .overlay { RoundedRectangle(cornerRadius: 9).strokeBorder(.white.opacity(0.09), lineWidth: 1) }
                }
                .buttonStyle(OverviewPressStyle())
                if process.isRunning {
                    Button(action: actions.restartServer) {
                        Image(systemName: "arrow.clockwise")
                            .font(.system(size: 12, weight: .medium))
                            .foregroundStyle(.secondary)
                            .frame(width: 30, height: 34)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(OverviewPressStyle())
                    .help("Restart server")
                    .accessibilityLabel("Restart server")
                }
            }
        }
        .frame(maxWidth: 390, alignment: .leading)
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

    private func systemRail(short: Bool) -> some View {
        HStack(alignment: .top, spacing: 16) {
            metric(title: "Uptime", value: online ? uptime : "—", symbol: "clock")
            railDivider
            metric(title: "Library", value: !online ? "Offline" : libraryReady ? "Available" : "Needs care", symbol: "rectangle.stack")
            railDivider
            Button { destination = .network } label: {
                metric(title: "Connection", value: connection, symbol: "network")
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help("View network addresses")
        }
        .padding(.vertical, short ? 17 : 22)
        .overlay(alignment: .top) { Rectangle().fill(.white.opacity(0.09)).frame(height: 1) }
        .overlay(alignment: .bottom) { Rectangle().fill(.white.opacity(0.09)).frame(height: 1) }
    }

    private var railDivider: some View {
        Rectangle().fill(.white.opacity(0.07)).frame(width: 1, height: 40)
            .padding(.horizontal, 2)
    }

    private func metric(title: String, value: String, symbol: String) -> some View {
        VStack(alignment: .leading, spacing: 9) {
            Label(title, systemImage: symbol)
                .font(.system(size: 10, weight: .medium)).foregroundStyle(.secondary)
            Text(value)
                .font(.system(size: 17, weight: .medium)).monospacedDigit().tracking(-0.4)
                .lineLimit(1).minimumScaleFactor(0.8)
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
