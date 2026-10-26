import AppKit
import SwiftUI

struct WelcomeOnboardingView: View {
    let port: Int
    var onFinish: () -> Void

    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var step = 0

    private var background: Color { Color(nsColor: MelloTVTheme.onboardingBackground) }
    private let titles = ["Welcome to Mello TV", "Your server", "Made independently."]
    private let subtitles = [
        "Movies and series. All in one place.",
        "Mello TV runs quietly in your menu bar.",
        "Ad-free, and built with care. Help support what comes next."
    ]

    var body: some View {
        GeometryReader { viewport in
            let horizontalInset = min(40, max(20, viewport.size.width * 0.06))
            let verticalInset = min(48, max(24, viewport.size.height * 0.06))

            ScrollView {
                VStack(spacing: 32) {
                    progress
                    pageContent
                    navigation
                }
                .frame(maxWidth: 400)
                .padding(.horizontal, horizontalInset)
                .padding(.vertical, verticalInset)
                .frame(maxWidth: .infinity)
                .frame(minHeight: viewport.size.height, alignment: .center)
            }
            .id(step)
        }
        .background(background.ignoresSafeArea())
        .foregroundStyle(.white)
        .preferredColorScheme(.dark)
    }

    private var progress: some View {
        HStack(spacing: 6) {
            ForEach(0..<3, id: \.self) { index in
                Capsule().fill(index <= step ? Color.white : Color.white.opacity(0.12))
                    .frame(width: 26, height: 3)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Step \(step + 1) of 3")
    }

    private var pageContent: some View {
        VStack(spacing: 0) {
            stepArtwork
                .accessibilityHidden(true)
                .padding(.bottom, 28)
            Text(titles[step])
                .font(.system(size: 28, weight: .semibold))
                .tracking(-0.8)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityAddTraits(.isHeader)
            Text(subtitles[step])
                .font(.system(size: 14))
                .foregroundStyle(.secondary)
                .lineSpacing(3)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 12)
                .padding(.bottom, 28)
            if step == 0 {
                VStack(alignment: .leading, spacing: 18) {
                    feature("film", title: "HD streaming")
                    feature("person.2", title: "Personal profiles")
                    feature("lock", title: "Private and ad-free")
                }
            } else if step == 1 {
                WelcomeServerConnectionView(port: port)
            } else {
                support
            }
        }
        .multilineTextAlignment(.center)
        .frame(maxWidth: .infinity)
    }

    @ViewBuilder
    private var stepArtwork: some View {
        if step == 0, let icon = NSApp.applicationIconImage {
            Image(nsImage: icon)
                .resizable()
                .interpolation(.high)
                .scaledToFit()
                .frame(width: 96, height: 96)
        } else {
            Image(systemName: step == 1 ? "menubar.rectangle" : "cup.and.saucer")
                .font(.system(size: 34, weight: .medium))
                .frame(width: 88, height: 88)
                .background {
                    RoundedRectangle(cornerRadius: 24, style: .continuous)
                        .fill(LinearGradient(colors: [.white.opacity(0.08), .white.opacity(0.025)],
                                             startPoint: .topLeading, endPoint: .bottomTrailing))
                        .overlay {
                            RoundedRectangle(cornerRadius: 24, style: .continuous)
                                .strokeBorder(.white.opacity(0.08), lineWidth: 1)
                        }
                }
        }
    }

    private func feature(_ icon: String, title: String) -> some View {
        HStack(spacing: 12) {
            Image(systemName: icon).frame(width: 20)
                .accessibilityHidden(true)
            Text(title).fixedSize(horizontal: false, vertical: true)
        }
            .font(.system(size: 14))
            .foregroundStyle(.secondary)
    }

    private var support: some View {
        VStack(spacing: 18) {
            Link(destination: AppLinks.buyMeACoffeeURL) {
                Text("☕ Buy Me a Coffee")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(Color(red: 0.14, green: 0.11, blue: 0))
                    .frame(maxWidth: .infinity, minHeight: 50)
                    .background(LinearGradient(colors: [Color(red: 1, green: 0.87, blue: 0), Color(red: 0.96, green: 0.73, blue: 0)],
                                               startPoint: .topLeading, endPoint: .bottomTrailing),
                                in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            }
            .buttonStyle(.plain)
            Text("Always optional. Thank you for being here.")
                .fixedSize(horizontal: false, vertical: true)
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
        }
    }

    private var navigation: some View {
        HStack(spacing: 0) {
            if step > 0 {
                Button {
                    move(to: step - 1)
                } label: {
                    Image(systemName: "chevron.left")
                        .font(.system(size: 14, weight: .semibold))
                        .frame(width: 48, height: 52)
                        .contentShape(Rectangle())
                }
                .accessibilityLabel("Back")
                Rectangle()
                    .fill(LinearGradient(colors: [.clear, .black.opacity(0.3), .clear],
                                         startPoint: .top, endPoint: .bottom))
                    .frame(width: 1, height: 22)
            }
            Button {
                if step < 2 { move(to: step + 1) }
                else { onFinish() }
            } label: {
                Text(step == 0 ? "Get started" : step == 1 ? "Continue" : "Open dashboard")
                    .font(.system(size: 14, weight: .semibold))
                    .frame(maxWidth: .infinity, minHeight: 52)
                    .padding(.trailing, step > 0 ? 49 : 0)
                    .contentShape(Rectangle())
            }
            .keyboardShortcut(.defaultAction)
        }
        .buttonStyle(.plain)
        .foregroundStyle(background)
        .background(.white, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .frame(maxWidth: 400)
    }

    private func move(to next: Int) {
        withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.18)) { step = next }
    }
}

private struct WelcomeServerConnectionView: View {
    let port: Int
    @State private var status: ServerStatus?
    @State private var checking = true
    @State private var showDetails = false

    var body: some View {
        VStack(spacing: 18) {
            HStack(spacing: 10) {
                Circle().fill(status == nil ? Color.gray : Color.green).frame(width: 7, height: 7)
                    .accessibilityHidden(true)
                Text(status != nil ? "Server running on this Mac" : checking ? "Starting your server…" : "Server is unavailable")
                    .font(.system(size: 14))
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(.horizontal, 14)
            .frame(maxWidth: .infinity, minHeight: 54)
            .background(.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 14))
            DisclosureGroup("Connect another device", isExpanded: $showDetails) {
                VStack(alignment: .leading, spacing: 18) {
                    Text("The iOS app finds your server on the same Wi-Fi.")
                        .foregroundStyle(.secondary)
                    addressGroup("This Mac", addresses: [AppLinks.clientURL(port: port).absoluteString])
                    let endpoints = status?.endpoints.compactMap(ServerEndpoint.init) ?? []
                    addressGroup("Home network", addresses: endpoints.filter { !$0.isRoaming }.map(\.address))
                    addressGroup("Tailscale", addresses: endpoints.filter(\.isRoaming).map(\.address))
                    if !endpoints.contains(where: \.isRoaming) {
                        Text("Connect Tailscale on both devices to watch away from home.")
                            .foregroundStyle(.secondary)
                    }
                }
                .font(.system(size: 12))
                .multilineTextAlignment(.leading)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.top, 16)
            }
            .font(.system(size: 13))
            .tint(.secondary)
        }
        .task {
            while !Task.isCancelled {
                do {
                    let url = AppLinks.clientURL(port: port).appendingPathComponent("api/v1/status")
                    let request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 5)
                    let (data, response) = try await URLSession.shared.data(for: request)
                    guard !Task.isCancelled else { return }
                    if (response as? HTTPURLResponse)?.statusCode == 200 {
                        let value = try JSONDecoder().decode(ServerStatus.self, from: data)
                        status = value.status == "ok" ? value : nil
                    } else { status = nil }
                } catch {
                    guard !Task.isCancelled else { return }
                    status = nil
                }
                checking = false
                do { try await Task.sleep(nanoseconds: 5_000_000_000) }
                catch { return }
            }
        }
    }

    @ViewBuilder
    private func addressGroup(_ title: String, addresses: [String]) -> some View {
        if !addresses.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                Text(title).fontWeight(.semibold)
                ForEach(addresses, id: \.self) { address in
                    Text(address).font(.system(size: 11, design: .monospaced))
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }
        }
    }
}

private struct ServerStatus: Decodable {
    let status: String
    let endpoints: [String]
}

private struct ServerEndpoint {
    let address: String
    let isRoaming: Bool

    init?(_ raw: String) {
        guard let url = URL(string: raw), let host = url.host,
              ["http", "https"].contains(url.scheme ?? ""),
              url.user == nil, url.password == nil,
              url.query == nil, url.fragment == nil,
              url.path.isEmpty || url.path == "/" else { return nil }
        address = raw
        let octets = host.split(separator: ".").compactMap { Int($0) }
        isRoaming = octets.count == 4 && octets[0] == 100 && (64...127).contains(octets[1])
    }
}
