import AppKit
import Network
import SwiftUI

// MARK: - Network Status Service

@MainActor
final class NetworkStatusService: ObservableObject {
    @Published var isTailscaleConnected: Bool = false
    @Published var isTailscaleInstalled: Bool = false
    @Published var activeVPNName: String? = nil
    @Published var isVPNActive: Bool = false
    @Published var primaryInterfaceName: String = "Wi-Fi"

    private let monitor = NWPathMonitor()
    private let queue = DispatchQueue(label: "NetworkStatusServiceQueue")

    init() {
        monitor.pathUpdateHandler = { [weak self] path in
            Task { @MainActor [weak self] in
                self?.evaluateNetwork(path: path)
            }
        }
        monitor.start(queue: queue)
        evaluateNetwork(path: monitor.currentPath)
    }

    func refresh() {
        evaluateNetwork(path: monitor.currentPath)
    }

    private func evaluateNetwork(path: NWPath) {
        // Detect primary interface
        if path.usesInterfaceType(.wifi) {
            primaryInterfaceName = "Wi-Fi"
        } else if path.usesInterfaceType(.wiredEthernet) {
            primaryInterfaceName = "Ethernet"
        } else {
            primaryInterfaceName = "Local Network"
        }

        // Query macOS system configuration for VPN & Tailscale
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let (tsConnected, tsInstalled, vpns) = Self.querySystemVPNs()
            DispatchQueue.main.async {
                self?.isTailscaleConnected = tsConnected
                self?.isTailscaleInstalled = tsInstalled
                self?.activeVPNName = vpns.first
                self?.isVPNActive = !vpns.isEmpty || path.usesInterfaceType(.other)
            }
        }
    }

    nonisolated private static func querySystemVPNs() -> (tailscale: Bool, tailscaleInstalled: Bool, activeVPNs: [String]) {
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/usr/sbin/scutil")
        task.arguments = ["--nc", "list"]
        let pipe = Pipe()
        task.standardOutput = pipe
        do {
            try task.run()
            task.waitUntilExit()
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            let output = String(decoding: data, as: UTF8.self)

            var tailscaleOn = false
            var tailscaleFound = false
            var vpns: [String] = []

            for line in output.components(separatedBy: "\n") {
                if line.contains("Tailscale") || line.contains("io.tailscale") {
                    tailscaleFound = true
                    if line.contains("(Connected)") {
                        tailscaleOn = true
                    }
                }
                if line.contains("(Connected)") {
                    if let first = line.range(of: "\""),
                       let second = line.range(of: "\"", range: first.upperBound..<line.endIndex) {
                        let name = String(line[first.upperBound..<second.lowerBound])
                        if !name.contains("Tailscale") {
                            vpns.append(name)
                        }
                    }
                }
            }
            return (tailscaleOn, tailscaleFound, vpns)
        } catch {
            return (false, false, [])
        }
    }
}

// MARK: - Main Network View

struct NetworkView: View {
    let port: Int
    let endpoints: [String]

    @StateObject private var statusService = NetworkStatusService()
    @State private var copiedID: String?
    @Environment(\.colorScheme) private var colorScheme

    private var portString: String {
        String(port)
    }

    private var interfaces: [NetworkInterfaceItem] {
        var items: [NetworkInterfaceItem] = []
        var seen = Set<String>()

        // 1. Home Wi-Fi & LAN IP
        for raw in endpoints {
            guard let url = URL(string: raw), let host = url.host,
                  !host.hasSuffix(".local"), host != "127.0.0.1", host != "localhost" else { continue }
            let octets = host.split(separator: ".").compactMap { Int($0) }
            let isTailscale = octets.count == 4 && octets[0] == 100 && (64...127).contains(octets[1])
            if !isTailscale && seen.insert(raw).inserted {
                items.append(NetworkInterfaceItem(
                    id: raw,
                    category: "Local Network",
                    name: "Home Wi-Fi & LAN",
                    address: raw,
                    subtitle: "Available to all iPhones, iPads, Apple TVs, and computers on your Wi-Fi",
                    icon: "wifi",
                    iconColor: Color(red: 0.25, green: 0.78, blue: 0.45)
                ))
            }
        }

        // 2. Bonjour Hostname (.local)
        for raw in endpoints {
            guard let url = URL(string: raw), let host = url.host, host.hasSuffix(".local") else { continue }
            if seen.insert(raw).inserted {
                items.append(NetworkInterfaceItem(
                    id: raw,
                    category: "Apple Bonjour",
                    name: "Zero-Config Hostname",
                    address: raw,
                    subtitle: "Apple companion apps discover this address automatically on your network",
                    icon: "antenna.radiowaves.left.and.right",
                    iconColor: Color(red: 0.65, green: 0.45, blue: 0.95)
                ))
            }
        }

        // 3. Localhost (This Mac)
        let local = "http://127.0.0.1:\(portString)"
        if seen.insert(local).inserted {
            items.append(NetworkInterfaceItem(
                id: local,
                category: "Local Loopback",
                name: "This Mac",
                address: local,
                subtitle: "Direct in-memory loopback stream for desktop player and browser testing",
                icon: "laptopcomputer",
                iconColor: Color(red: 0.25, green: 0.65, blue: 1.0)
            ))
        }

        return items
    }

    private var tailscaleEndpoint: String? {
        endpoints.first { raw in
            guard let url = URL(string: raw), let host = url.host else { return false }
            let octets = host.split(separator: ".").compactMap { Int($0) }
            return octets.count == 4 && octets[0] == 100 && (64...127).contains(octets[1])
        }
    }

    var body: some View {
        ScrollView(.vertical, showsIndicators: false) {
            VStack(spacing: 20) {
                // Network Hero Hub Banner
                networkHubHero
                    .padding(.top, 24)

                // Tailscale & VPN Mesh Tiles (Award-winning 2-Column Bento)
                HStack(spacing: 16) {
                    tailscaleBentoTile
                    vpnBentoTile
                }

                // Primary Network Endpoints Card
                endpointsCard

                // Companion Devices Guide
                companionDevicesBanner
                    .padding(.bottom, 36)
            }
            .frame(maxWidth: 760)
            .padding(.horizontal, 28)
            .frame(maxWidth: .infinity)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .onAppear {
            statusService.refresh()
        }
    }

    // MARK: - Network Hero Hub
    private var networkHubHero: some View {
        NetworkCard {
            HStack(spacing: 16) {
                ZStack {
                    Circle()
                        .fill(endpoints.isEmpty ? MelloTVTheme.sunken : Color(red: 0.25, green: 0.78, blue: 0.45).opacity(0.16))
                        .frame(width: 44, height: 44)

                    Image(systemName: "antenna.radiowaves.left.and.right")
                        .font(.system(size: 18, weight: .semibold))
                        .foregroundStyle(endpoints.isEmpty ? MelloTVTheme.muted : Color(red: 0.25, green: 0.78, blue: 0.45))
                }

                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 8) {
                        Text("Mello TV Broadcast Hub")
                            .font(.system(size: 15, weight: .bold))
                            .foregroundStyle(MelloTVTheme.text)

                        HStack(spacing: 5) {
                            Circle()
                                .fill(endpoints.isEmpty ? MelloTVTheme.mutedSubtle : Color(red: 0.25, green: 0.78, blue: 0.45))
                                .frame(width: 7, height: 7)
                            Text(endpoints.isEmpty ? "Offline" : "Broadcasting")
                                .font(.system(size: 11, weight: .semibold))
                                .foregroundStyle(endpoints.isEmpty ? MelloTVTheme.muted : Color(red: 0.25, green: 0.78, blue: 0.45))
                        }
                        .padding(.horizontal, 8)
                        .padding(.vertical, 3)
                        .background(
                            (endpoints.isEmpty ? MelloTVTheme.sunken : Color(red: 0.25, green: 0.78, blue: 0.45).opacity(0.12)),
                            in: Capsule()
                        )
                    }

                    Text("Zero-config DNS-SD service (_mellotv._tcp) active on \(statusService.primaryInterfaceName)")
                        .font(.system(size: 12))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                }

                Spacer(minLength: 12)

                // Port Pill - Exact numeric string format (No digit grouping commas!)
                HStack(spacing: 6) {
                    Text("Port")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                    Text(portString)
                        .font(.system(size: 13, weight: .bold, design: .monospaced))
                        .foregroundStyle(MelloTVTheme.text)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 6)
                .background(MelloTVTheme.sunken, in: Capsule())
                .overlay {
                    Capsule().strokeBorder(MelloTVTheme.border, lineWidth: 1)
                }
            }
            .padding(.vertical, 6)
        }
    }

    // MARK: - Tailscale Bento Tile
    private var tailscaleBentoTile: some View {
        NetworkCard {
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    ZStack {
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(statusService.isTailscaleConnected
                                ? Color(red: 0.25, green: 0.78, blue: 0.45).opacity(0.16)
                                : Color(red: 0.35, green: 0.55, blue: 0.95).opacity(0.16))
                            .frame(width: 34, height: 34)

                        Image(systemName: "globe.americas.fill")
                            .font(.system(size: 16, weight: .semibold))
                            .foregroundStyle(statusService.isTailscaleConnected
                                ? Color(red: 0.25, green: 0.78, blue: 0.45)
                                : Color(red: 0.35, green: 0.55, blue: 0.95))
                    }

                    VStack(alignment: .leading, spacing: 2) {
                        Text("Tailscale Mesh")
                            .font(.system(size: 13, weight: .bold))
                            .foregroundStyle(MelloTVTheme.text)
                        Text(statusService.isTailscaleConnected ? "Remote Online" : statusService.isTailscaleInstalled ? "Installed · Idle" : "Not Detected")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundStyle(statusService.isTailscaleConnected ? Color(red: 0.25, green: 0.78, blue: 0.45) : MelloTVTheme.textSecondary)
                    }

                    Spacer()

                    Circle()
                        .fill(statusService.isTailscaleConnected ? Color(red: 0.25, green: 0.78, blue: 0.45) : MelloTVTheme.mutedSubtle)
                        .frame(width: 8, height: 8)
                }

                if let ep = tailscaleEndpoint, statusService.isTailscaleConnected {
                    HStack {
                        Text(ep)
                            .font(.system(size: 11, weight: .medium, design: .monospaced))
                            .foregroundStyle(MelloTVTheme.textSecondary)
                            .lineLimit(1)
                        Spacer()
                        Button {
                            copyAddress(ep)
                        } label: {
                            Image(systemName: copiedID == ep ? "checkmark" : "doc.on.doc")
                                .font(.system(size: 10, weight: .semibold))
                                .foregroundStyle(copiedID == ep ? Color(red: 0.25, green: 0.78, blue: 0.45) : MelloTVTheme.text)
                                .frame(width: 24, height: 24)
                                .background(MelloTVTheme.sunken, in: RoundedRectangle(cornerRadius: 6))
                        }
                        .buttonStyle(.plain)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .background(MelloTVTheme.sunken, in: RoundedRectangle(cornerRadius: 8))
                } else {
                    Text(statusService.isTailscaleInstalled
                        ? "Connect Tailscale to securely stream movies outside your home network."
                        : "Install Tailscale for zero-configuration encrypted streaming away from home.")
                        .font(.system(size: 11))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .lineSpacing(2)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .padding(.vertical, 4)
        }
    }

    // MARK: - VPN Bento Tile
    private var vpnBentoTile: some View {
        NetworkCard {
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    ZStack {
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(statusService.isVPNActive
                                ? Color(red: 0.65, green: 0.45, blue: 0.95).opacity(0.16)
                                : MelloTVTheme.sunken)
                            .frame(width: 34, height: 34)

                        Image(systemName: statusService.isVPNActive ? "shield.lefthalf.filled" : "shield")
                            .font(.system(size: 16, weight: .semibold))
                            .foregroundStyle(statusService.isVPNActive
                                ? Color(red: 0.65, green: 0.45, blue: 0.95)
                                : MelloTVTheme.muted)
                    }

                    VStack(alignment: .leading, spacing: 2) {
                        Text("System VPN")
                            .font(.system(size: 13, weight: .bold))
                            .foregroundStyle(MelloTVTheme.text)
                        Text(statusService.isVPNActive
                            ? (statusService.activeVPNName ?? "Encrypted Tunnel Active")
                            : "Direct LAN Connection")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundStyle(statusService.isVPNActive ? Color(red: 0.65, green: 0.45, blue: 0.95) : MelloTVTheme.textSecondary)
                    }

                    Spacer()

                    Circle()
                        .fill(statusService.isVPNActive ? Color(red: 0.65, green: 0.45, blue: 0.95) : MelloTVTheme.mutedSubtle)
                        .frame(width: 8, height: 8)
                }

                Text(statusService.isVPNActive
                    ? "VPN active. If local devices cannot discover Mello TV, enable LAN bypass in VPN settings."
                    : "No system VPN tunnel detected. Local devices have direct low-latency access.")
                    .font(.system(size: 11))
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .lineSpacing(2)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(.vertical, 4)
        }
    }

    // MARK: - Endpoints Card
    private var endpointsCard: some View {
        NetworkCard {
            VStack(spacing: 0) {
                ForEach(Array(interfaces.enumerated()), id: \.element.id) { index, item in
                    if index > 0 {
                        Divider()
                            .overlay(MelloTVTheme.border)
                            .padding(.leading, 54)
                    }

                    HStack(spacing: 16) {
                        ZStack {
                            RoundedRectangle(cornerRadius: 10, style: .continuous)
                                .fill(item.iconColor.opacity(0.16))
                                .frame(width: 38, height: 38)
                            Image(systemName: item.icon)
                                .font(.system(size: 16, weight: .semibold))
                                .foregroundStyle(item.iconColor)
                        }

                        VStack(alignment: .leading, spacing: 3) {
                            HStack(spacing: 8) {
                                Text(item.name)
                                    .font(.system(size: 14, weight: .bold))
                                    .foregroundStyle(MelloTVTheme.text)

                                Text(item.category)
                                    .font(.system(size: 10, weight: .semibold))
                                    .foregroundStyle(MelloTVTheme.muted)
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 2)
                                    .background(MelloTVTheme.sunken, in: Capsule())
                            }

                            Text(item.subtitle)
                                .font(.system(size: 11))
                                .foregroundStyle(MelloTVTheme.textSecondary)
                                .lineLimit(1)
                        }

                        Spacer(minLength: 16)

                        // Trailing URL & Action Controls
                        HStack(spacing: 10) {
                            Text(item.address)
                                .font(.system(size: 12, weight: .medium, design: .monospaced))
                                .foregroundStyle(MelloTVTheme.text)
                                .padding(.horizontal, 10)
                                .padding(.vertical, 6)
                                .background(MelloTVTheme.sunken, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                                .overlay {
                                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                                        .strokeBorder(MelloTVTheme.border, lineWidth: 1)
                                }

                            Button {
                                copyAddress(item.address)
                            } label: {
                                Image(systemName: copiedID == item.address ? "checkmark" : "doc.on.doc")
                                    .font(.system(size: 12, weight: .semibold))
                                    .foregroundStyle(copiedID == item.address ? Color(red: 0.25, green: 0.78, blue: 0.45) : MelloTVTheme.text)
                                    .frame(width: 32, height: 32)
                                    .background(MelloTVTheme.sunken, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                                    .overlay {
                                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                                            .strokeBorder(copiedID == item.address ? Color(red: 0.25, green: 0.78, blue: 0.45).opacity(0.4) : MelloTVTheme.border, lineWidth: 1)
                                    }
                            }
                            .buttonStyle(.plain)
                            .help("Copy \(item.address)")

                            if let url = URL(string: item.address) {
                                Button {
                                    NSWorkspace.shared.open(url)
                                } label: {
                                    Image(systemName: "arrow.up.forward")
                                        .font(.system(size: 11, weight: .bold))
                                        .foregroundStyle(MelloTVTheme.textSecondary)
                                        .frame(width: 32, height: 32)
                                        .background(MelloTVTheme.sunken, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                                        .overlay {
                                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                                .strokeBorder(MelloTVTheme.border, lineWidth: 1)
                                        }
                                }
                                .buttonStyle(.plain)
                                .help("Open in browser")
                            }
                        }
                    }
                    .padding(.vertical, 10)
                    .frame(minHeight: 52)
                }
            }
        }
    }

    // MARK: - Companion Banner
    private var companionDevicesBanner: some View {
        HStack(spacing: 12) {
            Image(systemName: "checkmark.seal.fill")
                .font(.system(size: 14, weight: .semibold))
                .foregroundStyle(MelloTVTheme.accent)

            Text("The Mello TV iOS app automatically connects to this server over Bonjour without typing IP addresses.")
                .font(.system(size: 12))
                .foregroundStyle(MelloTVTheme.textSecondary)

            Spacer()

            if let copiedID {
                HStack(spacing: 5) {
                    Image(systemName: "checkmark")
                        .font(.system(size: 10, weight: .bold))
                    Text("Copied \(copiedID)")
                        .font(.system(size: 11, weight: .medium, design: .monospaced))
                        .lineLimit(1)
                }
                .foregroundStyle(Color(red: 0.25, green: 0.78, blue: 0.45))
                .padding(.horizontal, 10)
                .padding(.vertical, 4)
                .background(Color(red: 0.25, green: 0.78, blue: 0.45).opacity(0.12), in: Capsule())
                .transition(.opacity.combined(with: .scale(scale: 0.95)))
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(MelloTVTheme.sunken.opacity(0.6), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .strokeBorder(MelloTVTheme.border, lineWidth: 0.7)
        }
    }

    private func copyAddress(_ address: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(address, forType: .string)

        withAnimation(.spring(response: 0.25, dampingFraction: 0.8)) {
            copiedID = address
        }

        Task {
            try? await Task.sleep(nanoseconds: 1_800_000_000)
            withAnimation(.easeOut(duration: 0.2)) {
                if copiedID == address { copiedID = nil }
            }
        }
    }
}

// MARK: - Reusable UI Components

private struct NetworkCard<Content: View>: View {
    @ViewBuilder var content: Content
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        VStack(spacing: 0) {
            content
        }
        .padding(.horizontal, 22)
        .padding(.vertical, 14)
        .background(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .fill(MelloTVTheme.card)
        )
        .overlay {
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .strokeBorder(MelloTVTheme.border, lineWidth: 1)
        }
        .shadow(color: Color.black.opacity(colorScheme == .dark ? 0.3 : 0.06), radius: 8, y: 3)
    }
}

private struct NetworkInterfaceItem: Identifiable {
    let id: String
    let category: String
    let name: String
    let address: String
    let subtitle: String
    let icon: String
    let iconColor: Color
}
