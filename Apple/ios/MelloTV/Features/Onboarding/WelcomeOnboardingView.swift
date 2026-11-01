import SwiftUI
import AVFoundation
import VisionKit
import MelloKit

struct WelcomeOnboardingView: View {
    @Bindable var profileStore: ProfileStore
    var onFinish: () -> Void

    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @ScaledMetric(relativeTo: .largeTitle) private var titleSize = 44
    @FocusState private var addressIsFocused: Bool
    @State private var step = 0
    @State private var manualServerAddress = ""
    @State private var manualConnectError: String?
    @State private var isConnectingManual = false
    @State private var hasVerifiedConnection = false
    @State private var showsScanner = false
    @State private var showsArtworkCredits = false
    @State private var showsManualAddress = false

    private var isConnected: Bool {
        !profileStore.isLoading && profileStore.errorMessage == nil && (!profileStore.profiles.isEmpty || hasVerifiedConnection)
    }
    private var isBusy: Bool { isConnectingManual || profileStore.isLoading || profileStore.isDiscoveringServer }

    var body: some View {
        GeometryReader { geometry in
            let wide = geometry.size.width >= 700 && !dynamicTypeSize.isAccessibilitySize
            let inlineActions = dynamicTypeSize.isAccessibilitySize || geometry.size.height < 400
            ScrollView {
                Group {
                    if step == 0 {
                        welcome(wide: wide, compact: geometry.size.height < 700)
                    } else {
                        connection(wide: wide)
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

    private func welcome(wide: Bool, compact: Bool) -> some View {
        let layout = wide
            ? AnyLayout(HStackLayout(alignment: .center, spacing: 56))
            : AnyLayout(VStackLayout(alignment: .leading, spacing: 28))
        return layout {
            OnboardingLibraryArtwork()
                .frame(maxWidth: .infinity)
                .frame(height: wide ? 360 : (compact ? 230 : 290))
                .overlay(alignment: .topTrailing) {
                    Button { showsArtworkCredits = true } label: {
                        Image(systemName: "info.circle")
                            .font(.system(size: 15, weight: .regular))
                            .foregroundStyle(MelloTVTheme.textSecondary)
                            .frame(width: 44, height: 44)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Artwork credits")
                    .accessibilityHint("View poster creators, sources, and licenses")
                }
            VStack(alignment: .leading, spacing: 28) {
                VStack(alignment: .leading, spacing: 16) {
                    Text("Your library.\nReady to watch.")
                        .font(.system(size: titleSize, weight: .bold))
                        .tracking(-1.8)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityAddTraits(.isHeader)
                    Text("Movies and series from your Mello TV server, right here on your iPhone.")
                        .font(.body)
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .lineSpacing(3)
                        .fixedSize(horizontal: false, vertical: true)
                }
                VStack(alignment: .leading, spacing: 16) {
                    welcomeFeature("rectangle.stack", title: "Browse your collection", detail: "Movies, series, and your saved favorites.")
                    welcomeFeature("play.circle", title: "Keep your place", detail: "Continue watching with your own profile.")
                }
                if wide { navigation }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func welcomeFeature(_ symbol: String, title: String, detail: String) -> some View {
        HStack(alignment: .top, spacing: 14) {
            Image(systemName: symbol)
                .font(.title3.weight(.regular))
                .foregroundStyle(MelloTVTheme.textSecondary)
                .frame(width: 26, height: 28)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 4) {
                Text(title).font(.subheadline.weight(.semibold))
                Text(detail).font(.subheadline).foregroundStyle(MelloTVTheme.textSecondary)
            }
        }
        .fixedSize(horizontal: false, vertical: true)
        .accessibilityElement(children: .combine)
    }

    private func connection(wide: Bool) -> some View {
        let layout = wide
            ? AnyLayout(HStackLayout(alignment: .top, spacing: 48))
            : AnyLayout(VStackLayout(alignment: .leading, spacing: 28))
        return layout {
            VStack(alignment: .leading, spacing: 28) {
                VStack(alignment: .leading, spacing: 12) {
                    Text(isConnected ? "Make yourself at home." : "Bring your library.")
                        .font(.largeTitle.bold())
                        .tracking(-1.2)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityAddTraits(.isHeader)
                    Text(isConnected ? "Your server is connected. Choose a profile next and start browsing."
                         : "Connect to the Mello TV server you use. Scan its code or choose one on your network.")
                        .font(.body)
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .lineSpacing(3)
                        .fixedSize(horizontal: false, vertical: true)
                }
                nearbyServers
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            VStack(alignment: .leading, spacing: 24) {
                Button {
                    addressIsFocused = false
                    showsScanner = true
                } label: {
                    HStack(alignment: .center, spacing: 18) {
                        Image(systemName: "qrcode.viewfinder")
                            .font(.system(size: 34, weight: .light))
                            .accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Scan server code").font(.headline)
                            Text("Find the QR code in Mello TV on your Mac.")
                                .font(.subheadline)
                                .foregroundStyle(MelloTVTheme.textSecondary)
                        }
                        .fixedSize(horizontal: false, vertical: true)
                        Spacer(minLength: 0)
                        Image(systemName: "chevron.right").font(.caption.weight(.semibold))
                            .accessibilityHidden(true)
                    }
                    .frame(maxWidth: .infinity, minHeight: 72, alignment: .leading)
                    .padding(20)
                    .contentShape(Rectangle())
                }
                .buttonStyle(AppleNeumorphicButtonStyle())
                .disabled(isBusy)

                DisclosureGroup(isExpanded: $showsManualAddress) {
                    manualConnection.padding(.top, 16)
                } label: {
                    Text("Enter an address instead").font(.subheadline.weight(.semibold))
                        .frame(minHeight: 44)
                }
                .tint(MelloTVTheme.textSecondary)

                if let message = manualConnectError ?? profileStore.errorMessage, !isBusy {
                    Text(message)
                        .font(.footnote)
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityLabel("Connection error: \(message)")
                }
                if wide { navigation }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var nearbyServers: some View {
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
    }

    private var manualConnection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Server address").font(.headline)
            Text("Use the address shown in your server’s Network settings.")
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
            PasteButton(payloadType: String.self) { values in
                guard let value = values.first else { return }
                manualServerAddress = value
                manualConnectError = nil
            }
            .tint(MelloTVTheme.textSecondary)
            .accessibilityLabel("Paste server address")
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

    private var navigation: some View {
        VStack(spacing: 12) {
            Button {
                if step == 0 { move(to: 1) } else { onFinish() }
            } label: {
                HStack(spacing: 10) {
                    Text(step == 0 ? "Connect your server" : "Open library")
                    Image(systemName: "arrow.right").font(.subheadline.weight(.semibold))
                }
                .font(.headline)
                .foregroundStyle(MelloTVTheme.background)
                .frame(maxWidth: .infinity, minHeight: 56)
                .contentShape(Rectangle())
            }
            .background(MelloTVTheme.text, in: .rect(cornerRadius: 18))
            .buttonStyle(CardScaleButtonStyle(scale: 0.98))
            .disabled(step == 1 && (!isConnected || isBusy))
            if step == 0 {
                Text("A Mello TV server is required.")
                    .font(.footnote)
                    .foregroundStyle(MelloTVTheme.textSecondary)
            } else {
                Button("Back") { move(to: 0) }
                    .font(.subheadline.weight(.medium))
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .frame(minHeight: 44)
                    .buttonStyle(.plain)
            }
        }
    }

    private func move(to next: Int) {
        addressIsFocused = false
        withAnimation(reduceMotion ? nil : .easeInOut(duration: 0.2)) { step = next }
    }

    private func receiveServerCode(_ value: String) {
        guard let components = URLComponents(string: value.trimmingCharacters(in: .whitespacesAndNewlines)),
              ["http", "https"].contains(components.scheme?.lowercased() ?? ""),
              components.user == nil, components.password == nil,
              components.path.isEmpty || components.path == "/",
              components.query == nil, components.fragment == nil,
              let url = ServerAddress.normalizeServerURL(value) else {
            manualConnectError = "This code doesn’t contain a server address. Scan the QR code shown by your Mello TV server."
            return
        }
        manualServerAddress = url.absoluteString
        showsManualAddress = true
        connectManually()
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

/// The complete licensed posters retain their titles, credits, and original proportions.
private struct OnboardingLibraryArtwork: View {
    var body: some View {
        GeometryReader { geometry in
            let posterHeight = min(geometry.size.height * 0.92, geometry.size.width / 1.5)
            let posterWidth = posterHeight * 0.707
            ZStack {
                poster(.bigBuckBunny, height: posterHeight)
                    .rotationEffect(.degrees(-13))
                    .offset(x: -posterWidth * 0.45, y: 5)
                    .scaleEffect(0.88)
                poster(.elephantsDream, height: posterHeight)
                    .rotationEffect(.degrees(12))
                    .offset(x: posterWidth * 0.45, y: 5)
                    .scaleEffect(0.88)
                poster(.sintel, height: posterHeight)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .accessibilityHidden(true)
    }

    private func poster(_ film: OnboardingPoster, height: CGFloat) -> some View {
        Image("onboarding-" + film.rawValue)
            .resizable()
            .scaledToFit()
            .frame(width: height * 0.707, height: height)
            .shadow(color: .black.opacity(0.22), radius: 18, x: 0, y: 14)
    }
}

private enum OnboardingPoster: String, CaseIterable, Identifiable {
    case sintel
    case bigBuckBunny = "big-buck-bunny"
    case elephantsDream = "elephants-dream"

    var id: Self { self }

    var title: String {
        switch self {
        case .sintel: "Sintel"
        case .bigBuckBunny: "Big Buck Bunny"
        case .elephantsDream: "Elephants Dream"
        }
    }

    var credit: String {
        switch self {
        case .sintel: "© Blender Foundation | durian.blender.org"
        case .bigBuckBunny: "© Blender Foundation | www.bigbuckbunny.org"
        case .elephantsDream: "Bassam Kurdali, Andy Goralczyk, and the Blender Foundation"
        }
    }

    var licenseVersion: String { self == .elephantsDream ? "4.0" : "3.0" }

    var source: URL {
        switch self {
        case .sintel: URL(string: "https://commons.wikimedia.org/wiki/File:Sintel_poster.jpg")!
        case .bigBuckBunny: URL(string: "https://commons.wikimedia.org/wiki/File:Big_buck_bunny_poster_big.jpg")!
        case .elephantsDream: URL(string: "https://commons.wikimedia.org/wiki/File:ElephantsDreamPoster.jpg")!
        }
    }
}

private struct OnboardingArtworkCredits: View {
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Text("Posters are shown as illustrations. The titles you can watch depend on your connected server.")
                        .foregroundStyle(.secondary)
                }
                ForEach(OnboardingPoster.allCases) { film in
                    Section(film.title) {
                        Text(film.credit)
                        Link("Original poster and attribution", destination: film.source)
                        Link("Creative Commons Attribution " + film.licenseVersion,
                             destination: URL(string: "https://creativecommons.org/licenses/by/" + film.licenseVersion + "/")!)
                        Text("Scaled down and arranged for display. The poster image is shown in full. No endorsement of Mello TV is implied.")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle("Artwork credits")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
            }
        }
    }
}

private struct ServerCodeScannerSheet: View {
    var onEnterAddress: () -> Void
    var onScan: (String) -> Void

    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    @State private var isReady = false
    @State private var message: String?
    @State private var showsSettings = false

    var body: some View {
        NavigationStack {
            VStack(spacing: 20) {
                if isReady {
                    ServerCodeCamera(onScan: onScan) { _ in
                        isReady = false
                        message = "The camera couldn’t start. You can enter your server address instead."
                    }
                    .clipShape(.rect(cornerRadius: 24))
                    Text("Point your camera at the QR code shown in Mello TV on your Mac. Scanning connects to that server.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                } else if let message {
                    ContentUnavailableView {
                        Label("Scan server code", systemImage: "qrcode.viewfinder")
                    } description: {
                        Text(message)
                    } actions: {
                        if showsSettings {
                            Button("Open Settings") {
                                if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
                            }
                        }
                        Button("Enter address instead", action: onEnterAddress)
                    }
                } else {
                    ProgressView("Preparing camera")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .padding(24)
            .navigationTitle("Scan server code")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
            .task { await prepareCamera() }
        }
    }

    private func prepareCamera() async {
        guard DataScannerViewController.isSupported else {
            message = "QR scanning needs a supported iPhone camera. You can still choose a nearby server or enter its address."
            return
        }
        let authorized: Bool
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized: authorized = true
        case .notDetermined: authorized = await AVCaptureDevice.requestAccess(for: .video)
        default: authorized = false
        }
        guard !Task.isCancelled else { return }
        guard authorized else {
            message = "Allow camera access in Settings to scan your server’s code, or enter its address instead."
            showsSettings = true
            return
        }
        guard DataScannerViewController.isAvailable else {
            message = "The camera is unavailable right now. Try again, or enter your server address instead."
            return
        }
        isReady = true
    }
}

private struct ServerCodeCamera: UIViewControllerRepresentable {
    var onScan: (String) -> Void
    var onFailure: (Error) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(onScan: onScan, onFailure: onFailure) }

    func makeUIViewController(context: Context) -> DataScannerViewController {
        let scanner = DataScannerViewController(
            recognizedDataTypes: [.barcode(symbologies: [.qr])],
            qualityLevel: .balanced,
            recognizesMultipleItems: false,
            isHighFrameRateTrackingEnabled: false,
            isPinchToZoomEnabled: true,
            isGuidanceEnabled: true,
            isHighlightingEnabled: true
        )
        scanner.delegate = context.coordinator
        return scanner
    }

    func updateUIViewController(_ scanner: DataScannerViewController, context: Context) {
        guard !scanner.isScanning, !context.coordinator.finished else { return }
        do { try scanner.startScanning() }
        catch {
            context.coordinator.finished = true
            Task { @MainActor in onFailure(error) }
        }
    }

    static func dismantleUIViewController(_ scanner: DataScannerViewController, coordinator: Coordinator) {
        scanner.stopScanning()
        scanner.delegate = nil
    }

    final class Coordinator: NSObject, DataScannerViewControllerDelegate {
        var finished = false
        let onScan: (String) -> Void
        let onFailure: (Error) -> Void

        init(onScan: @escaping (String) -> Void, onFailure: @escaping (Error) -> Void) {
            self.onScan = onScan
            self.onFailure = onFailure
        }

        func dataScanner(_ scanner: DataScannerViewController, didAdd addedItems: [RecognizedItem], allItems: [RecognizedItem]) {
            guard !finished else { return }
            for item in addedItems {
                if case .barcode(let barcode) = item, let value = barcode.payloadStringValue {
                    finished = true
                    scanner.stopScanning()
                    onScan(value)
                    return
                }
            }
        }

        func dataScanner(_ scanner: DataScannerViewController, becameUnavailableWithError error: DataScannerViewController.ScanningUnavailable) {
            guard !finished else { return }
            finished = true
            scanner.stopScanning()
            onFailure(error)
        }
    }
}
