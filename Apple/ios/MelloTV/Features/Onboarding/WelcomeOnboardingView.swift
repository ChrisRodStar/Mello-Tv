import MelloKit
import SwiftUI

struct WelcomeOnboardingView: View {
    // MARK: - Inputs

    @Bindable var profileStore: ProfileStore
    var onFinish: () -> Void

    // MARK: - Environment

    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    // MARK: - Onboarding State

    @FocusState private var addressIsFocused: Bool
    @State private var step = 0
    @State private var manualServerAddress = ""
    @State private var manualConnectError: String?
    @State private var isConnectingManual = false
    @State private var hasVerifiedConnection = false
    @State private var showsScanner = false
    @State private var showsArtworkCredits = false
    @State private var showsManualAddress = false

    // MARK: - Connection Status

    private var isConnected: Bool {
        !profileStore.isLoading && profileStore.errorMessage == nil
            && (!profileStore.profiles.isEmpty || hasVerifiedConnection)
    }

    private var isBusy: Bool {
        isConnectingManual || profileStore.isLoading || profileStore.isDiscoveringServer
    }

    // MARK: - Page Layout

    var body: some View {
        GeometryReader { geometry in
            let wide = geometry.size.width >= 700 && !dynamicTypeSize.isAccessibilitySize
            let inlineActions = dynamicTypeSize.isAccessibilitySize || geometry.size.height < 400
            ScrollView {
                Group {
                    if step == 0 {
                        OnboardingWelcomeScreen(
                            wide: wide,
                            compact: geometry.size.height < 700,
                            onShowCredits: { showsArtworkCredits = true },
                            navigation: navigation
                        )
                    } else {
                        OnboardingConnectionScreen(
                            profileStore: profileStore,
                            wide: wide,
                            isConnected: isConnected,
                            isBusy: isBusy,
                            isConnectingManual: isConnectingManual,
                            manualServerAddress: $manualServerAddress,
                            manualConnectError: $manualConnectError,
                            showsManualAddress: $showsManualAddress,
                            addressIsFocused: $addressIsFocused,
                            onScan: showScanner,
                            onSearch: searchForServers,
                            onSelect: selectServer,
                            onConnect: connectManually,
                            navigation: navigation
                        )
                    }
                }
                .padding(.horizontal, wide ? 40 : 28)
                .padding(.top, wide ? 32 : 20)
                .padding(.bottom, 24)
                .frame(maxWidth: wide ? 1040 : 540)
                .frame(maxWidth: .infinity)
                .frame(minHeight: wide ? geometry.size.height : nil)

                if inlineActions && !wide {
                    navigation.padding(.horizontal, 28).padding(.bottom, 24)
                }
            }
            .scrollBounceBehavior(.basedOnSize)
            .scrollDismissesKeyboard(.interactively)
            .id(step)
            .safeAreaInset(edge: .bottom, spacing: 0) {
                if !inlineActions && !wide {
                    navigation
                        .padding(.horizontal, 28)
                        .padding(.top, 16)
                        .padding(.bottom, 16)
                        .frame(maxWidth: 540)
                        .frame(maxWidth: .infinity)
                        .background(MelloTVTheme.background.opacity(0.96))
                }
            }
        }
        .background {
            MelloTVTheme.background.ignoresSafeArea()
            CatalogPageSurface().opacity(0.45).ignoresSafeArea()
        }
        .foregroundStyle(MelloTVTheme.text)
        .sheet(isPresented: $showsScanner) {
            ServerCodeScannerSheet(onEnterAddress: {
                showsScanner = false
                showsManualAddress = true
            }) { value in
                showsScanner = false
                receiveServerCode(value)
            }
        }
        .sheet(isPresented: $showsArtworkCredits) {
            OnboardingArtworkCredits()
        }
        .task {
            if manualServerAddress.isEmpty, let saved = ServerAddress.tailscaleURL {
                manualServerAddress = saved.absoluteString
            }
        }
        .task(id: step) {
            if step == 1 && profileStore.discoveredServers.isEmpty && !isBusy && !isConnected {
                await profileStore.scanForServers()
            }
        }
    }

    // MARK: - Navigation

    private var navigation: some View {
        OnboardingNavigation(
            isWelcome: step == 0,
            canContinue: isConnected && !isBusy,
            onContinue: {
                if step == 0 { move(to: 1) } else { onFinish() }
            },
            onBack: { move(to: 0) }
        )
    }

    // MARK: - Actions

    private func showScanner() {
        addressIsFocused = false
        showsScanner = true
    }

    private func searchForServers() {
        Task { await profileStore.scanForServers() }
    }

    private func selectServer(_ server: DiscoveredServer) {
        addressIsFocused = false
        Task {
            await profileStore.selectDiscoveredServer(server)
            hasVerifiedConnection = profileStore.errorMessage == nil
        }
    }

    private func move(to next: Int) {
        addressIsFocused = false
        withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.2)) { step = next }
    }

    private func receiveServerCode(_ value: String) {
        guard
            let components = URLComponents(
                string: value.trimmingCharacters(in: .whitespacesAndNewlines)),
            ["http", "https"].contains(components.scheme?.lowercased() ?? ""),
            components.user == nil, components.password == nil,
            components.path.isEmpty || components.path == "/",
            components.query == nil, components.fragment == nil,
            let url = ServerAddress.normalizeServerURL(value)
        else {
            manualConnectError =
                "This code doesn’t contain a server address. Scan the QR code shown by your Mello TV server."
            return
        }
        manualServerAddress = url.absoluteString
        showsManualAddress = true
        connectManually()
    }

    private func connectManually() {
        guard !manualServerAddress.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, !isBusy
        else { return }
        addressIsFocused = false
        manualConnectError = nil
        isConnectingManual = true
        Task {
            defer { isConnectingManual = false }
            do {
                try await profileStore.connectToCustomServer(manualServerAddress)
                hasVerifiedConnection = profileStore.errorMessage == nil
            } catch { manualConnectError = error.localizedDescription }
        }
    }
}
