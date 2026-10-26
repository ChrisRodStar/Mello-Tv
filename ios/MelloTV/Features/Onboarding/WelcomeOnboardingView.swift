import SwiftUI

struct WelcomeOnboardingView: View {
    @Bindable var profileStore: ProfileStore
    var onFinish: () -> Void

    @Environment(\.openURL) private var openURL
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @FocusState private var addressIsFocused: Bool
    @State private var step = 0
    @State private var manualServerAddress = ""
    @State private var manualConnectError: String?
    @State private var isConnectingManual = false
    @State private var hasVerifiedConnection = false

    private let steps = ["Welcome", "Your server", "Made independently"]
    private var isConnected: Bool {
        !profileStore.isLoading && profileStore.errorMessage == nil && (!profileStore.profiles.isEmpty || hasVerifiedConnection)
    }
    private var isBusy: Bool { isConnectingManual || profileStore.isLoading || profileStore.isDiscoveringServer }

    var body: some View {
        GeometryReader { geometry in
            let compact = geometry.size.height < 600
            let inlineActions = dynamicTypeSize.isAccessibilitySize || geometry.size.height < 350
            ScrollView {
                VStack(alignment: .leading, spacing: compact ? 28 : 40) {
                    header
                    if step == 0 {
                        welcome(compact: compact)
                    } else if step == 1 {
                        connection
                    } else {
                        support(compact: compact)
                    }
                    if inlineActions { navigation }
                }
                .padding(.horizontal, 28)
                .padding(.top, 12)
                .padding(.bottom, 28)
                .frame(maxWidth: 540, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .scrollBounceBehavior(.basedOnSize)
            .scrollDismissesKeyboard(.interactively)
            .id(step)
            .safeAreaInset(edge: .bottom, spacing: 0) {
                if !inlineActions {
                    navigation
                        .padding(.horizontal, 28)
                        .padding(.top, 20)
                        .padding(.bottom, 16)
                        .frame(maxWidth: 540)
                        .frame(maxWidth: .infinity)
                        .background {
                            LinearGradient(colors: [MelloTVTheme.background.opacity(0), MelloTVTheme.background,
                                                    MelloTVTheme.background], startPoint: .top, endPoint: .bottom)
                                .ignoresSafeArea(edges: .bottom)
                        }
                }
            }
        }
        .background { CatalogPageSurface().ignoresSafeArea() }
        .foregroundStyle(MelloTVTheme.text)
        .task {
            if manualServerAddress.isEmpty {
                #if canImport(UIKit)
                if let clip = UIPasteboard.general.string,
                   let url = ServerAddress.normalizeServerURL(clip) {
                    manualServerAddress = url.absoluteString
                } else if let ts = ServerAddress.tailscaleURL {
                    manualServerAddress = ts.absoluteString
                }
                #endif
            }
            if profileStore.discoveredServers.isEmpty && !isBusy && !isConnected {
                await profileStore.scanForServers()
            }
        }
    }

    private var header: some View {
        HStack(spacing: 10) {
            PhosphorIcon("brand-tv-mark", size: 23)
                .accessibilityHidden(true)
            Text("Mello TV")
                .font(.headline)
                .tracking(-0.5)
            Spacer()
            Text("\(step + 1) / \(steps.count)")
                .font(.caption.weight(.medium).monospacedDigit())
                .foregroundStyle(MelloTVTheme.textSecondary)
                .accessibilityLabel("Step \(step + 1) of \(steps.count): \(steps[step])")
        }
        .frame(minHeight: 44)
    }

    private func introduction(_ title: String, detail: String) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title)
                .font(.largeTitle.weight(.bold))
                .tracking(-1.2)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityAddTraits(.isHeader)
            Text(detail)
                .font(.body)
                .foregroundStyle(MelloTVTheme.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
                .lineSpacing(3)
        }
    }

    private func emblem(_ icon: String, compact: Bool) -> some View {
        Group {
            if icon == "brand-tv-mark" {
                PhosphorIcon(icon, size: compact ? 46 : 64)
            } else {
                Image(systemName: "heart.fill")
                    .font(.system(size: compact ? 42 : 58, weight: .medium))
            }
        }
            .foregroundStyle(MelloTVTheme.text)
            .frame(width: compact ? 100 : 136, height: compact ? 100 : 136)
            .appleNeumorphic(cornerRadius: compact ? 26 : 34, elevation: .prominent)
            .frame(maxWidth: .infinity)
            .padding(.vertical, compact ? 8 : 24)
            .accessibilityHidden(true)
    }

    private func welcome(compact: Bool) -> some View {
        VStack(alignment: .leading, spacing: compact ? 28 : 36) {
            emblem("brand-tv-mark", compact: compact)
            introduction("Settle in.", detail: "Your movies, your series. A space for everything you love to watch.")
            VStack(spacing: 22) {
                feature("film-slate", title: "Find your next favorite", detail: "Movies and series, all in one place.")
                feature("users", title: "Make it yours", detail: "Personal profiles. Recommendations for you.")
                feature("play", title: "Pick up where you left off", detail: "Your watchlist and progress, together.")
            }
        }
    }

    private func feature(_ icon: String, title: String, detail: String) -> some View {
        HStack(alignment: .top, spacing: 16) {
            PhosphorIcon(icon, size: 23)
                .foregroundStyle(MelloTVTheme.textSecondary)
                .frame(width: 28, height: 28)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 4) {
                Text(title).font(.subheadline.weight(.semibold))
                Text(detail).font(.subheadline).foregroundStyle(MelloTVTheme.textSecondary)
            }
            .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
    }

    private var connection: some View {
        VStack(alignment: .leading, spacing: 28) {
            introduction(isConnected ? "You're connected." : "Bring your library.",
                         detail: isConnected ? "Your server is ready. Make yourself at home."
                         : "Connect to your Mello TV server to get started. We'll look on your home network.")
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Text(isConnected ? "Your server" : "Nearby servers").font(.subheadline.weight(.semibold))
                    Spacer()
                    Button {
                        Task { await profileStore.scanForServers() }
                    } label: {
                        Text(profileStore.isDiscoveringServer ? "Searching…" : "Search again")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(MelloTVTheme.textSecondary)
                            .touchTarget()
                    }
                    .disabled(isBusy)
                    .buttonStyle(.plain)
                }
                if isConnected {
                    serverRow(name: profileStore.connectedServerName,
                              address: profileStore.connectedServerAddress, connected: true)
                } else {
                    ForEach(profileStore.discoveredServers) { server in
                        Button {
                            addressIsFocused = false
                            Task {
                                await profileStore.selectDiscoveredServer(server)
                                hasVerifiedConnection = profileStore.errorMessage == nil
                            }
                        } label: {
                            serverRow(name: server.name, address: server.primaryURL.absoluteString, connected: false)
                        }
                        .buttonStyle(CardScaleButtonStyle())
                        .disabled(isBusy)
                    }
                    if profileStore.discoveredServers.isEmpty {
                        HStack(alignment: .top, spacing: 14) {
                            if isBusy {
                                ProgressView().tint(MelloTVTheme.text)
                            } else {
                                PhosphorIcon("wifi-slash", size: 22).accessibilityHidden(true)
                            }
                            VStack(alignment: .leading, spacing: 5) {
                                Text(isBusy ? "Looking for your server" : "No server found yet")
                                    .font(.subheadline.weight(.semibold))
                                Text(isBusy ? "This may take a few moments." : "Check that your server is running and you're on the same Wi-Fi or connected via Tailscale.")
                                    .font(.subheadline)
                                    .foregroundStyle(MelloTVTheme.textSecondary)
                            }
                            .fixedSize(horizontal: false, vertical: true)
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(20)
                        .appleNeumorphicRecessed(cornerRadius: 16)
                    }
                }
            }
            Rectangle().fill(MelloTVTheme.subtleBorder).frame(height: 1).accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 12) {
                Text("Have a server address?").font(.subheadline.weight(.semibold))
                Text("Enter a local address or your Tailscale address.")
                    .font(.subheadline).foregroundStyle(MelloTVTheme.textSecondary)
                TextField("192.168.1.50:8080", text: $manualServerAddress)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.URL)
                    .submitLabel(.go)
                    .focused($addressIsFocused)
                    .onSubmit { connectManually() }
                    .padding(16)
                    .appleNeumorphicRecessed(cornerRadius: 12)
                    .accessibilityLabel("Server address")
                Button(action: connectManually) {
                    HStack(spacing: 10) {
                        if isConnectingManual { ProgressView().tint(MelloTVTheme.text) }
                        Text(isConnectingManual ? "Connecting…" : "Connect to server")
                            .font(.subheadline.weight(.semibold))
                    }
                    .frame(maxWidth: .infinity, minHeight: 50)
                    .contentShape(Rectangle())
                }
                .buttonStyle(AppleNeumorphicButtonStyle())
                .disabled(manualServerAddress.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || isBusy)
                if let message = manualConnectError ?? profileStore.errorMessage, !isBusy {
                    Text(message)
                        .font(.footnote)
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityLabel("Connection error: \(message)")
                }
            }
        }
    }

    private func serverRow(name: String, address: String, connected: Bool) -> some View {
        HStack(spacing: 14) {
            PhosphorIcon("television", size: 26).accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 5) {
                Text(name).font(.subheadline.weight(.semibold))
                Text(address).font(.caption).foregroundStyle(MelloTVTheme.textSecondary)
                    .lineLimit(2).truncationMode(.middle)
                if connected {
                    Text("Connected").font(.caption.weight(.medium)).foregroundStyle(MelloTVTheme.accent)
                }
            }
            Spacer(minLength: 0)
            PhosphorIcon(connected ? "check-circle" : "caret-right", size: 22)
                .foregroundStyle(connected ? MelloTVTheme.accent : MelloTVTheme.textSecondary)
                .accessibilityHidden(true)
        }
        .foregroundStyle(MelloTVTheme.text)
        .multilineTextAlignment(.leading)
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .appleNeumorphic(cornerRadius: 16, elevation: .subtle)
        .accessibilityElement(children: .combine)
    }

    private func support(compact: Bool) -> some View {
        VStack(alignment: .leading, spacing: 32) {
            emblem("heart", compact: compact)
            introduction("A little independent TV.", detail: "Mello TV is made independently, with care. Thanks for making it part of your downtime.")
            VStack(alignment: .leading, spacing: 20) {
                Text("If you'd like to support future updates, you can buy the creator a coffee. It's always optional.")
                    .font(.subheadline)
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
                    .lineSpacing(3)
                Button { openURL(AppLinks.buyMeACoffeeURL) } label: {
                    HStack(spacing: 10) {
                        Text("Support Mello TV").font(.subheadline.weight(.semibold))
                        PhosphorIcon("caret-right", size: 16)
                    }
                    .frame(maxWidth: .infinity, minHeight: 52)
                    .contentShape(Rectangle())
                }
                .buttonStyle(AppleNeumorphicButtonStyle())
                Button { openURL(AppLinks.websiteURL) } label: {
                    Text("Visit mellowatch.com")
                        .font(.subheadline)
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .frame(maxWidth: .infinity, minHeight: 44)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
    }

    private var navigation: some View {
        VStack(spacing: 14) {
            HStack(spacing: 8) {
                ForEach(steps.indices, id: \.self) { index in
                    Capsule()
                        .fill(index == step ? MelloTVTheme.text : MelloTVTheme.subtleBorder)
                        .frame(width: 24, height: 3)
                }
            }
            .accessibilityHidden(true)
            HStack(spacing: 0) {
                if step > 0 {
                    Button { move(to: step - 1) } label: {
                        PhosphorIcon("caret-left", size: 18, weight: .bold)
                            .foregroundStyle(MelloTVTheme.background.opacity(0.8))
                            .frame(width: 52, height: 56)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(CardScaleButtonStyle(scale: 0.94))
                    .accessibilityLabel("Back")

                    Capsule()
                        .fill(
                            LinearGradient(
                                colors: [MelloTVTheme.background.opacity(0.08),
                                         MelloTVTheme.background.opacity(0.38),
                                         MelloTVTheme.background.opacity(0.08)],
                                startPoint: .top,
                                endPoint: .bottom
                            )
                        )
                        .frame(width: 1, height: 26)
                        .accessibilityHidden(true)
                }
                Button {
                    if step < steps.count - 1 { move(to: step + 1) } else { onFinish() }
                } label: {
                    HStack(spacing: 10) {
                        Text(step == 0 ? "Get started" : step == 1 ? (isConnected ? "Continue" : "Set up later") : "Open Mello TV")
                        PhosphorIcon("caret-right", size: 16, weight: .bold)
                    }
                    .font(.headline)
                    .foregroundStyle(MelloTVTheme.background)
                    .frame(maxWidth: .infinity, minHeight: 56)
                    .padding(.horizontal, 16)
                    .contentShape(Rectangle())
                }
                .buttonStyle(CardScaleButtonStyle(scale: 0.98))
            }
            .background(MelloTVTheme.text, in: .rect(cornerRadius: 16))
            .clipShape(.rect(cornerRadius: 16))
            .disabled(isConnectingManual)
        }
    }

    private func move(to next: Int) {
        addressIsFocused = false
        withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.2)) { step = next }
    }

    private func connectManually() {
        guard !manualServerAddress.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, !isBusy else { return }
        addressIsFocused = false
        manualConnectError = nil
        isConnectingManual = true
        Task {
            defer { isConnectingManual = false }
            do {
                try await profileStore.connectToCustomServer(manualServerAddress)
                hasVerifiedConnection = profileStore.errorMessage == nil
            }
            catch { manualConnectError = error.localizedDescription }
        }
    }
}
