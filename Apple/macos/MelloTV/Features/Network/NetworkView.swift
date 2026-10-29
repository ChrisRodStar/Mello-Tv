import AppKit
import CoreImage
import Darwin
import Network
import SwiftUI
import MelloKit

/// Uses the active physical interface to prefer a useful LAN address over virtual adapters.
@MainActor
private final class NetworkStatusService: ObservableObject {
    @Published private(set) var preferredHosts: Set<String> = []
    private let monitor = NWPathMonitor()

    init() {
        monitor.pathUpdateHandler = { [weak self] path in
            Task { @MainActor [weak self] in
                self?.preferredHosts = Self.physicalHosts(for: path)
            }
        }
        monitor.start(queue: DispatchQueue(label: "MelloTV.NetworkPath"))
    }

    deinit { monitor.cancel() }

    private static func physicalHosts(for path: NWPath) -> Set<String> {
        let names = Set(path.availableInterfaces.filter {
            ($0.type == .wifi || $0.type == .wiredEthernet) && path.usesInterfaceType($0.type)
        }.map(\.name))
        var list: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&list) == 0, let first = list else { return [] }
        defer { freeifaddrs(list) }
        var hosts = Set<String>()
        var current: UnsafeMutablePointer<ifaddrs>? = first
        while let entry = current {
            defer { current = entry.pointee.ifa_next }
            guard names.contains(String(cString: entry.pointee.ifa_name)),
                  let address = entry.pointee.ifa_addr,
                  address.pointee.sa_family == UInt8(AF_INET) || address.pointee.sa_family == UInt8(AF_INET6)
            else { continue }
            var buffer = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            if getnameinfo(address, socklen_t(address.pointee.sa_len), &buffer,
                           socklen_t(buffer.count), nil, 0, NI_NUMERICHOST) == 0 {
                hosts.insert(String(cString: buffer).lowercased())
            }
        }
        return hosts
    }
}

private struct NetworkAddress: Identifiable {
    let url: URL
    let host: String
    var id: String { url.absoluteString }
    var isTailscale: Bool {
        if host.hasSuffix(".ts.net") || host.hasPrefix("fd7a:115c:a1e0:") { return true }
        let parts = host.split(separator: ".").compactMap { Int($0) }
        return parts.count == 4 && parts[0] == 100 && (64...127).contains(parts[1])
    }
    var kind: String {
        if host.hasSuffix(".ts.net") { return "MagicDNS" }
        if host.hasSuffix(".local") { return "Bonjour" }
        return host.contains(":") ? "IPv6" : "IPv4"
    }

    init?(_ raw: String) {
        guard let url = URL(string: raw), url.scheme == "http" || url.scheme == "https",
              let host = url.host?.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: "[]")),
              host != "localhost", host != "::1", !host.hasPrefix("127.") else { return nil }
        self.url = url
        self.host = host
    }

    func localPriority(preferredHosts: Set<String>) -> Int {
        if preferredHosts.contains(host) { return host.contains(":") ? 1 : 0 }
        if host.hasSuffix(".local") { return 2 }
        return host.contains(":") ? 4 : 3
    }
}

@MainActor
struct NetworkView: View {
    let port: Int
    let endpoints: [String]
    let online: Bool
    let starting: Bool
    @StateObject private var status = NetworkStatusService()
    @State private var copiedAddress: String?
    @State private var copyGeneration = UUID()
    @State private var qrAddress: NetworkAddress?
    @Environment(\.colorScheme) private var colorScheme

    private var addresses: [NetworkAddress] {
        guard online else { return [] }
        var seen = Set<String>()
        return endpoints.compactMap(NetworkAddress.init).filter { seen.insert($0.id).inserted }
    }
    private var localAddresses: [NetworkAddress] {
        addresses.filter { !$0.isTailscale }.sorted {
            let left = $0.localPriority(preferredHosts: status.preferredHosts)
            let right = $1.localPriority(preferredHosts: status.preferredHosts)
            return left == right ? $0.id < $1.id : left < right
        }
    }
    private var remoteAddresses: [NetworkAddress] {
        addresses.filter(\.isTailscale).sorted {
            // Stable IPv4 addresses also work when a device does not use MagicDNS.
            let left = $0.kind == "IPv4" ? 0 : $0.kind == "MagicDNS" ? 1 : 2
            let right = $1.kind == "IPv4" ? 0 : $1.kind == "MagicDNS" ? 1 : 2
            return left == right ? $0.id < $1.id : left < right
        }
    }

    var body: some View {
        GeometryReader { viewport in
            let compact = viewport.size.height < 620
            ScrollView {
                VStack(alignment: .leading, spacing: compact ? 22 : 28) {
                    introduction(compact: compact)
                    VStack(spacing: 0) {
                        connection(
                            title: "At home", icon: "wifi", caption: "Same Wi-Fi or Ethernet network",
                            addresses: localAddresses,
                            instruction: "Open Mello TV on your device and choose this server. For a browser, use the address below.",
                            empty: "No local address reported. Connect this Mac to Wi-Fi or Ethernet."
                        )
                        Rectangle().fill(MelloTVTheme.border.opacity(0.55))
                            .frame(height: 1).padding(.horizontal, 26)
                        connection(
                            title: "Away from home", icon: "globe", caption: "Through Tailscale",
                            addresses: remoteAddresses,
                            instruction: "Connect this Mac and your device to the same Tailscale network, then use this address.",
                            empty: "Connect Tailscale on this Mac. Its address will appear when the server reports it."
                        )
                    }
                    .background {
                        RoundedRectangle(cornerRadius: 28, style: .continuous)
                            .fill(LinearGradient(stops: [
                                .init(color: MelloTVTheme.surfaceHighlight, location: 0),
                                .init(color: MelloTVTheme.card, location: 0.5),
                                .init(color: MelloTVTheme.surfaceShade, location: 1)
                            ], startPoint: .topLeading, endPoint: .bottomTrailing))
                            .overlay {
                                RoundedRectangle(cornerRadius: 28, style: .continuous)
                                    .strokeBorder(LinearGradient(
                                        colors: [.white.opacity(colorScheme == .dark ? 0.10 : 0.85), MelloTVTheme.subtleBorder],
                                        startPoint: .topLeading, endPoint: .bottomTrailing
                                    ), lineWidth: 1)
                            }
                            .shadow(color: MelloTVTheme.ambientShadow.opacity(colorScheme == .dark ? 0.25 : 0.10),
                                    radius: 18, y: 12)
                    }
                    thisMac
                }
                .padding(compact ? 26 : 34)
                .frame(maxWidth: 960, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
        }
        .task(id: copyGeneration) {
            guard copiedAddress != nil else { return }
            do { try await Task.sleep(nanoseconds: 2_000_000_000) }
            catch { return }
            copiedAddress = nil
        }
        .onChange(of: online) { isOnline in
            copiedAddress = nil
            if !isOnline { qrAddress = nil }
        }
        .sheet(item: $qrAddress) { address in
            AddressQRSheet(address: address)
        }
    }

    private func introduction(compact: Bool) -> some View {
        VStack(alignment: .leading, spacing: 9) {
            Text(starting ? "Getting ready to connect." : online ? "Connect your devices." : "Your server is offline.")
                .font(.system(size: compact ? 27 : 32, weight: .semibold))
                .tracking(-1)
                .foregroundStyle(MelloTVTheme.text)
                .accessibilityAddTraits(.isHeader)
            Text(starting ? "Connection addresses appear when your server is ready." : online
                 ? "One address for home. One for everywhere else."
                 : "Start your server from Overview to connect another screen.")
                .font(.system(size: 12))
                .foregroundStyle(MelloTVTheme.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.horizontal, 4)
    }

    private func connection(title: String, icon: String, caption: String,
                            addresses: [NetworkAddress], instruction: String, empty: String) -> some View {
        VStack(alignment: .leading, spacing: 17) {
            HStack(spacing: 13) {
                Image(systemName: icon)
                    .font(.system(size: 20, weight: .medium))
                    .foregroundStyle(online && !addresses.isEmpty ? MelloTVTheme.accent : MelloTVTheme.muted)
                    .frame(width: 44, height: 44)
                    .background(MelloTVTheme.sunken.opacity(0.65), in: RoundedRectangle(cornerRadius: 14))
                    .overlay {
                        RoundedRectangle(cornerRadius: 14).strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 1)
                    }
                VStack(alignment: .leading, spacing: 4) {
                    Text(title).font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(MelloTVTheme.text)
                        .accessibilityAddTraits(.isHeader)
                    Text(caption).font(.system(size: 11))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                }
                Spacer(minLength: 0)
                Text(starting ? "Starting" : !online ? "Offline" : addresses.isEmpty ? "No address" : "Address available")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(online && !addresses.isEmpty ? MelloTVTheme.accent : MelloTVTheme.muted)
            }
            if let primary = addresses.first {
                addressBar(primary)
                if addresses.count > 1 {
                    DisclosureGroup("Other addresses · \(addresses.count - 1)") {
                        VStack(spacing: 8) {
                            ForEach(Array(addresses.dropFirst())) { address in
                                addressBar(address, secondary: true)
                            }
                        }
                        .padding(.top, 10)
                    }
                    .font(.system(size: 11))
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .tint(MelloTVTheme.textSecondary)
                }
                Text(instruction).font(.system(size: 11)).lineSpacing(3)
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                Text(starting ? "Waiting for your server to finish starting." : online ? empty : "Available when your server is running.")
                    .font(.system(size: 12)).lineSpacing(3)
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .frame(maxWidth: .infinity, minHeight: 42, alignment: .leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(24)
    }

    private func addressBar(_ address: NetworkAddress, secondary: Bool = false) -> some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                if secondary {
                    Text(address.kind).font(.system(size: 10, weight: .medium))
                        .foregroundStyle(MelloTVTheme.muted)
                }
                Text(address.id)
                    .font(.system(size: secondary ? 11 : 13, weight: .medium, design: .monospaced))
                    .foregroundStyle(MelloTVTheme.text)
                    .textSelection(.enabled)
                    .lineLimit(1).truncationMode(.middle)
                    .help(address.id)
            }
            Spacer(minLength: 0)
            Button {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(address.id, forType: .string)
                copiedAddress = address.id
                copyGeneration = UUID()
            } label: {
                Label(copiedAddress == address.id ? "Copied" : "Copy", systemImage: copiedAddress == address.id ? "checkmark" : "doc.on.doc")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(MelloTVTheme.text)
                    .padding(.horizontal, 12).frame(height: 32)
                    .background(MelloTVTheme.cardHover, in: RoundedRectangle(cornerRadius: 10))
                    .overlay {
                        RoundedRectangle(cornerRadius: 10).strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 1)
                    }
            }
            .buttonStyle(OverviewPressStyle())
            .accessibilityLabel("Copy \(address.kind) address")
            Button {
                qrAddress = address
            } label: {
                Label("QR code", systemImage: "qrcode")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(MelloTVTheme.text)
                    .padding(.horizontal, 12).frame(height: 32)
                    .background(MelloTVTheme.cardHover, in: RoundedRectangle(cornerRadius: 10))
                    .overlay {
                        RoundedRectangle(cornerRadius: 10).strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 1)
                    }
            }
            .buttonStyle(OverviewPressStyle())
            .accessibilityLabel("Show QR code for \(address.kind) address")
            Button {
                NSWorkspace.shared.open(address.url)
            } label: {
                Image(systemName: "arrow.up.right")
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .frame(width: 30, height: 32).contentShape(Rectangle())
            }
            .buttonStyle(OverviewPressStyle())
            .help("Open \(address.id) in your browser")
            .accessibilityLabel("Open \(address.kind) address in browser")
        }
        .padding(.horizontal, 14).padding(.vertical, secondary ? 9 : 13)
        .background(MelloTVTheme.sunken.opacity(0.7), in: RoundedRectangle(cornerRadius: 14))
        .overlay {
            RoundedRectangle(cornerRadius: 14).strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 1)
        }
    }

    private var thisMac: some View {
        HStack(spacing: 12) {
            Image(systemName: "desktopcomputer").font(.system(size: 17, weight: .regular))
                .foregroundStyle(MelloTVTheme.muted)
            VStack(alignment: .leading, spacing: 4) {
                Text("Watching on this Mac?").font(.system(size: 12, weight: .medium))
                    .foregroundStyle(MelloTVTheme.text)
                Text("Local player · Port \(String(port))").font(.system(size: 11))
                    .foregroundStyle(MelloTVTheme.textSecondary)
            }
            Spacer(minLength: 8)
            Button {
                if let url = URL(string: "http://127.0.0.1:\(port)") { NSWorkspace.shared.open(url) }
            } label: {
                Label("Open web player", systemImage: "arrow.up.right")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(online ? MelloTVTheme.accent : MelloTVTheme.mutedSubtle)
                    .padding(.vertical, 8)
            }
            .buttonStyle(OverviewPressStyle()).disabled(!online)
        }
        .padding(.horizontal, 4)
    }

}

private struct AddressQRSheet: View {
    let address: NetworkAddress
    @Environment(\.dismiss) private var dismiss

    private var qrImage: NSImage? {
        guard let filter = CIFilter(name: "CIQRCodeGenerator") else { return nil }
        filter.setValue(Data(address.id.utf8), forKey: "inputMessage")
        filter.setValue("M", forKey: "inputCorrectionLevel")
        guard let output = filter.outputImage?.transformed(by: CGAffineTransform(scaleX: 10, y: 10)),
              let image = CIContext().createCGImage(output, from: output.extent) else { return nil }
        return NSImage(cgImage: image, size: NSSize(width: 220, height: 220))
    }

    var body: some View {
        VStack(spacing: 18) {
            Text("Connect to Mello TV").font(.headline)
            if let qrImage {
                Image(nsImage: qrImage)
                    .interpolation(.none)
                    .resizable()
                    .frame(width: 220, height: 220)
                    .padding(16)
                    .background(.white, in: RoundedRectangle(cornerRadius: 12))
                    .accessibilityLabel("QR code for \(address.id)")
            } else {
                Text("QR code unavailable. Use the address below instead.")
                    .font(.callout).foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
            Text(address.id).font(.system(.caption, design: .monospaced)).textSelection(.enabled)
            Text("Scan with your device’s camera to open this address in a browser. In the Mello TV app, enter the address when connecting.")
                .font(.callout).foregroundStyle(.secondary)
                .multilineTextAlignment(.center).fixedSize(horizontal: false, vertical: true)
            Button("Done") { dismiss() }.keyboardShortcut(.defaultAction)
        }
        .padding(28).frame(width: 360)
    }
}
