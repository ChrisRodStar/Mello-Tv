import SwiftUI
import MelloKit

struct OverviewView: View {
    @ObservedObject var model: ServerStatusStore
    @ObservedObject var process: ServerProcessState
    let actions: AppActions
    @Binding var destination: DesktopDestination

    private var online: Bool { process.isRunning && model.isReachable }
    private var libraryReady: Bool { online && model.snapshot?.catalogStatus == "ready" && model.snapshot?.catalogSnapshot == "retained" }
    private var libraryNew: Bool { online && model.snapshot?.catalogStatus == "pending" }
    private var mediaReady: Bool { online && model.snapshot?.media.configured == true }
    private var catalogUsable: Bool { libraryReady || libraryNew }
    private var ready: Bool { online && catalogUsable && mediaReady }

    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        GeometryReader { viewport in
            let compact = viewport.size.height < 600
            VStack(alignment: .leading, spacing: compact ? 22 : 28) {
                serverPanel(compact: compact)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                HStack {
                    Button { destination = .network } label: {
                        Label("Connect a device", systemImage: "iphone")
                            .font(.system(size: 12, weight: .medium))
                            .foregroundStyle(MelloTVTheme.textSecondary)
                            .padding(.vertical, 8)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(OverviewPressStyle())
                    Spacer()
                    if online {
                        Text("Running for \(uptime)")
                            .font(.system(size: 11, weight: .medium))
                            .monospacedDigit()
                            .foregroundStyle(MelloTVTheme.textSecondary)
                    }
                }
                .padding(.horizontal, 8)
            }
            .padding(compact ? 26 : 34)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
    }

    private func serverPanel(compact: Bool) -> some View {
        VStack(alignment: .leading, spacing: compact ? 24 : 34) {
            HStack(spacing: 8) {
                Circle()
                    .fill(statusTint)
                    .frame(width: 6, height: 6)
                    .shadow(color: statusTint.opacity(online ? 0.3 : 0), radius: 4)
                    .accessibilityHidden(true)
                Text(online ? "Running on this Mac" : process.isRunning ? "Connecting to this Mac" : "Server stopped")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(MelloTVTheme.textSecondary)
                Spacer()
                if process.isRunning { serverControls }
            }

            VStack(alignment: .leading, spacing: compact ? 22 : 28) {
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
                        .frame(maxWidth: 480, alignment: .leading)
                }
                primaryControl
            }
            .padding(.horizontal, compact ? 4 : 8)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)

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
        Button(action: actions.toggleServer) {
            Label("Stop server", systemImage: "power")
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(MelloTVTheme.textSecondary)
                .padding(.horizontal, 13)
                .frame(height: 34)
                .background(MelloTVTheme.sunken.opacity(0.65), in: Capsule())
                .overlay { Capsule().strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 1) }
        }
        .buttonStyle(OverviewPressStyle())
        .help("Stop the local server")
    }

    @ViewBuilder
    private var primaryControl: some View {
        if !process.isRunning {
            overviewAction("Start server", symbol: "power", action: actions.toggleServer)
        } else if online {
            if !mediaReady || model.snapshot?.catalogStatus == "unavailable" {
                overviewAction("Open logs", symbol: "doc.text.magnifyingglass", action: actions.openLogs)
            } else if !catalogUsable {
                overviewAction("Open settings", symbol: "gearshape", action: { destination = .settings })
            } else {
                overviewAction("Open web player", symbol: "play.fill", action: actions.openWebsite)
            }
        } else if process.isRunning, model.error != nil {
            overviewAction("Open logs", symbol: "doc.text.magnifyingglass", action: actions.openLogs)
        }
    }

    private func overviewAction(_ title: String, symbol: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Label(title, systemImage: symbol)
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(Color.white)
                .padding(.horizontal, 18)
                .frame(height: 42)
                .background(MelloTVTheme.accent, in: RoundedRectangle(cornerRadius: 13, style: .continuous))
                .overlay {
                    RoundedRectangle(cornerRadius: 13, style: .continuous)
                        .strokeBorder(.white.opacity(0.16), lineWidth: 1)
                }
                .shadow(color: MelloTVTheme.accent.opacity(0.15), radius: 8, y: 4)
        }
        .buttonStyle(OverviewPressStyle())
    }

    private var systemRail: some View {
        HStack(alignment: .center, spacing: 18) {
            metric(title: "Playback", value: playback)
                .help("Streams delivering data now or within the last three minutes, including buffered playback.")
            railDivider
            metric(title: "Library", value: libraryStatus)
            railDivider
            Button { destination = .network } label: {
                HStack(spacing: 8) {
                    metric(title: "Network", value: connection)
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
        if !online || !ready { return .orange }
        return MelloTVTheme.accent
    }

    private var headline: String {
        if !process.isRunning { return "Your cinema,\non pause." }
        if !online { return model.error == nil ? "Waking up\nyour cinema." : "Let's reconnect\nyour cinema." }
        if !catalogUsable || !mediaReady { return "Your server\nneeds attention." }
        return "Your cinema\nis online."
    }

    private var detail: String {
        if !process.isRunning { return "Start your server to watch on your iPhone or in the web player." }
        if !online { return model.error ?? "Getting your library and connections ready." }
        if !mediaReady { return "The playback engine is unavailable. Open the logs to investigate." }
        if model.snapshot?.catalogStatus == "unavailable" {
            return "Your library couldn't be opened. Open the logs to investigate."
        }
        if !catalogUsable {
            return model.snapshot?.catalogStatus == "unconfigured"
                ? "Your catalog isn't configured. Open settings to set it up."
                : "Your library is unavailable. Open settings to check its configuration."
        }
        if libraryNew { return "Open the web player to browse. Your library fills as you explore." }
        return "Browse in the web player, or open Mello TV on your iPhone."
    }

    private var playback: String {
        guard online else { return "Offline" }
        guard mediaReady else { return "Unavailable" }
        guard let count = model.activeStreams else { return "Unavailable" }
        return count == 0 ? "Idle" : "\(count) stream\(count == 1 ? "" : "s")"
    }

    private var libraryStatus: String {
        if !online { return "Offline" }
        if libraryReady { return "Ready" }
        if libraryNew { return "Ready to browse" }
        return model.snapshot?.catalogStatus == "unconfigured" ? "Set up needed" : "Unavailable"
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
