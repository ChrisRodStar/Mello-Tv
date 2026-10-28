import SwiftUI

struct WhoIsWatchingView: View {
    @Bindable var profileStore: ProfileStore
    @State private var manualServerInput: String = ""
    @State private var manualConnectError: String?
    @State private var isConnectingManual: Bool = false

    private let columns = [
        GridItem(.adaptive(minimum: 120, maximum: 160), spacing: 28)
    ]

    var body: some View {
        ZStack(alignment: .topTrailing) {
            MelloTVTheme.background
                .ignoresSafeArea()

            if profileStore.isLoading && profileStore.profiles.isEmpty {
                VStack(spacing: 16) {
                    ProgressView()
                        .tint(MelloTVTheme.accent)
                        .scaleEffect(1.2)
                    Text(profileStore.isDiscoveringServer ? "Finding Mello TV on your network…" : "Loading profiles…")
                        .font(.subheadline.weight(.medium))
                        .foregroundStyle(MelloTVTheme.muted)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if profileStore.profiles.isEmpty && profileStore.errorMessage != nil {
                serverConnectionState
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if profileStore.profiles.isEmpty {
                emptyState
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                content
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }

            serverStatusPill
                .padding(.top, 12)
                .padding(.trailing, 20)
        }
        .sheet(isPresented: $profileStore.showProfileEditor) {
            ProfileEditorView(profileStore: profileStore)
        }
        .sheet(isPresented: $profileStore.showServerConnectionSheet) {
            ServerConnectionSheet(profileStore: profileStore)
        }
    }

    private var serverStatusPill: some View {
        Button {
            Task {
                await profileStore.scanForServers()
            }
            profileStore.showServerConnectionSheet = true
        } label: {
            HStack(spacing: 8) {
                Circle()
                    .fill(profileStore.errorMessage == nil ? Color.green : Color.orange)
                    .frame(width: 8, height: 8)
                Text(profileStore.connectedServerName)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(MelloTVTheme.text)
                    .lineLimit(1)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
            .background(MelloTVTheme.cardBackground.opacity(0.9), in: Capsule())
            .overlay(
                Capsule()
                    .stroke(MelloTVTheme.border, lineWidth: 1)
            )
        }
        .buttonStyle(CardScaleButtonStyle(scale: 0.96))
    }

    private var content: some View {
        VStack(spacing: 36) {
            Spacer()

            VStack(spacing: 10) {
                Text(profileStore.isManagingProfiles ? "Manage Profiles" : "Who's Watching?")
                    .font(.system(size: 34, weight: .bold, design: .rounded))
                    .foregroundStyle(MelloTVTheme.text)

                if profileStore.isManagingProfiles {
                    Text("Select a profile to edit its name, color, or preferences.")
                        .font(.subheadline)
                        .foregroundStyle(MelloTVTheme.muted)
                }
            }

            LazyVGrid(columns: columns, spacing: 32) {
                ForEach(profileStore.profiles) { profile in
                    profileButton(for: profile)
                }

                if profileStore.profiles.count < 6 {
                    addProfileButton
                }
            }
            .padding(.horizontal, 32)
            .frame(maxWidth: 600)

            Spacer()

            Button {
                withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) {
                    profileStore.isManagingProfiles.toggle()
                }
            } label: {
                Text(profileStore.isManagingProfiles ? "Done" : "Manage Profiles")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(MelloTVTheme.text)
                    .padding(.horizontal, 28)
                    .padding(.vertical, 12)
                    .background(MelloTVTheme.cardBackground, in: RoundedRectangle(cornerRadius: 8))
                    .overlay(
                        RoundedRectangle(cornerRadius: 8)
                            .stroke(MelloTVTheme.border, lineWidth: 1)
                    )
            }
            .buttonStyle(CardScaleButtonStyle(scale: 0.96))
            .padding(.bottom, 36)
        }
    }

    private func profileButton(for profile: Profile) -> some View {
        Button {
            if profileStore.isManagingProfiles {
                profileStore.openEditProfile(profile)
            } else {
                profileStore.selectProfile(profile)
            }
        } label: {
            VStack(spacing: 12) {
                ZStack {
                    ProfileAvatarView(name: profile.name, colorHex: profile.color, size: 104)

                    if profileStore.isManagingProfiles {
                        RoundedRectangle(cornerRadius: 18, style: .continuous)
                            .fill(.black.opacity(0.6))
                            .frame(width: 104, height: 104)

                        PhosphorIcon("pencil-simple", size: 24, weight: .bold)
                            .foregroundStyle(.white)
                    }
                }

                Text(profile.name)
                    .font(.callout.weight(.medium))
                    .foregroundStyle(MelloTVTheme.text)
                    .lineLimit(1)
            }
        }
        .buttonStyle(CardScaleButtonStyle(scale: 0.94))
    }

    private var addProfileButton: some View {
        Button {
            profileStore.openCreateProfile()
        } label: {
            VStack(spacing: 12) {
                ZStack {
                    RoundedRectangle(cornerRadius: 18, style: .continuous)
                        .fill(MelloTVTheme.cardBackground)
                        .frame(width: 104, height: 104)
                        .overlay(
                            RoundedRectangle(cornerRadius: 18, style: .continuous)
                                .stroke(MelloTVTheme.border, lineWidth: 1.5)
                        )

                    PhosphorIcon("plus", size: 28, weight: .bold)
                        .foregroundStyle(MelloTVTheme.muted)
                }

                Text("Add Profile")
                    .font(.callout.weight(.medium))
                    .foregroundStyle(MelloTVTheme.muted)
            }
        }
        .buttonStyle(CardScaleButtonStyle(scale: 0.94))
    }

    private var serverConnectionState: some View {
        ScrollView {
            VStack(spacing: 24) {
                Spacer(minLength: 32)

                ZStack {
                    Circle()
                        .fill(MelloTVTheme.accent.opacity(0.14))
                        .frame(width: 84, height: 84)
                    PhosphorIcon("television", size: 40, weight: .fill)
                        .foregroundStyle(MelloTVTheme.accent)
                }

                VStack(spacing: 8) {
                    Text("Connect to Mello TV")
                        .font(.title2.weight(.bold))
                        .foregroundStyle(MelloTVTheme.text)

                    Text("Mello TV automatically searches your local Wi-Fi network. Make sure the Mello TV app is running on your Mac or home server, or enter its address below.")
                        .font(.subheadline)
                        .foregroundStyle(MelloTVTheme.muted)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 24)
                }

                if !profileStore.discoveredServers.isEmpty {
                    VStack(alignment: .leading, spacing: 10) {
                        Text("DISCOVERED ON YOUR NETWORK")
                            .font(.caption.weight(.bold))
                            .foregroundStyle(MelloTVTheme.muted)

                        ForEach(profileStore.discoveredServers) { server in
                            Button {
                                Task {
                                    await profileStore.selectDiscoveredServer(server)
                                }
                            } label: {
                                HStack(spacing: 12) {
                                    Circle()
                                        .fill(Color.green)
                                        .frame(width: 10, height: 10)
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(server.name)
                                            .font(.subheadline.weight(.semibold))
                                            .foregroundStyle(MelloTVTheme.text)
                                        Text(server.primaryURL.absoluteString)
                                            .font(.caption)
                                            .foregroundStyle(MelloTVTheme.muted)
                                    }
                                    Spacer()
                                    Text("Connect")
                                        .font(.caption.weight(.bold))
                                        .foregroundStyle(MelloTVTheme.accent)
                                }
                                .padding(14)
                                .background(MelloTVTheme.cardBackground, in: RoundedRectangle(cornerRadius: 12))
                                .overlay(
                                    RoundedRectangle(cornerRadius: 12)
                                        .stroke(MelloTVTheme.border, lineWidth: 1)
                                )
                            }
                            .buttonStyle(CardScaleButtonStyle(scale: 0.98))
                        }
                    }
                    .frame(maxWidth: 420)
                    .padding(.horizontal, 24)
                }

                Button {
                    Task {
                        await profileStore.scanForServers()
                    }
                } label: {
                    HStack(spacing: 10) {
                        if profileStore.isDiscoveringServer {
                            ProgressView()
                                .tint(.white)
                        } else {
                            PhosphorIcon("arrow-counter-clockwise", size: 18, weight: .bold)
                        }
                        Text(profileStore.isDiscoveringServer ? "Scanning Network…" : "Scan Network Again")
                            .font(.headline)
                    }
                    .foregroundStyle(.white)
                    .frame(maxWidth: 360)
                    .padding(.vertical, 14)
                    .background(MelloTVTheme.accent, in: RoundedRectangle(cornerRadius: 10))
                }
                .disabled(profileStore.isDiscoveringServer)
                .buttonStyle(CardScaleButtonStyle(scale: 0.97))
                .padding(.horizontal, 24)

                VStack(alignment: .leading, spacing: 10) {
                    Text("MANUAL SERVER OR TAILSCALE ADDRESS")
                        .font(.caption.weight(.bold))
                        .foregroundStyle(MelloTVTheme.muted)

                    HStack(spacing: 10) {
                        TextField("e.g. 192.168.1.50:8080 or 100.x.y.z:8080", text: $manualServerInput)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .keyboardType(.URL)
                            .font(.subheadline)
                            .foregroundStyle(MelloTVTheme.text)
                            .padding(.horizontal, 14)
                            .padding(.vertical, 12)
                            .background(MelloTVTheme.cardBackground, in: RoundedRectangle(cornerRadius: 10))
                            .overlay(
                                RoundedRectangle(cornerRadius: 10)
                                    .stroke(MelloTVTheme.border, lineWidth: 1)
                            )

                        Button {
                            Task {
                                manualConnectError = nil
                                isConnectingManual = true
                                do {
                                    try await profileStore.connectToCustomServer(manualServerInput)
                                } catch {
                                    manualConnectError = error.localizedDescription
                                }
                                isConnectingManual = false
                            }
                        } label: {
                            if isConnectingManual {
                                ProgressView()
                                    .tint(.white)
                                    .padding(.horizontal, 16)
                                    .padding(.vertical, 12)
                            } else {
                                Text("Connect")
                                    .font(.subheadline.weight(.bold))
                                    .foregroundStyle(.white)
                                    .padding(.horizontal, 18)
                                    .padding(.vertical, 12)
                            }
                        }
                        .background(MelloTVTheme.cardBackground, in: RoundedRectangle(cornerRadius: 10))
                        .overlay(
                            RoundedRectangle(cornerRadius: 10)
                                .stroke(MelloTVTheme.border, lineWidth: 1)
                        )
                        .disabled(manualServerInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || isConnectingManual)
                    }

                    if let manualConnectError {
                        Text(manualConnectError)
                            .font(.caption)
                            .foregroundStyle(.red)
                    }
                }
                .frame(maxWidth: 420)
                .padding(.horizontal, 24)

                Spacer(minLength: 32)
            }
        }
    }

    private var emptyState: some View {
        VStack(spacing: 20) {
            PhosphorIcon("users", size: 60, weight: .fill)
                .foregroundStyle(MelloTVTheme.muted)

            Text("No Profiles Found")
                .font(.title2.weight(.bold))
                .foregroundStyle(MelloTVTheme.text)

            Text("Create a profile to personalize your family streaming experience.")
                .font(.subheadline)
                .foregroundStyle(MelloTVTheme.muted)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)

            Button {
                profileStore.openCreateProfile()
            } label: {
                Text("Add Profile")
                    .font(.headline)
                    .foregroundStyle(.white)
                    .padding(.horizontal, 32)
                    .padding(.vertical, 14)
                    .background(MelloTVTheme.accent, in: RoundedRectangle(cornerRadius: 8))
            }
            .buttonStyle(CardScaleButtonStyle(scale: 0.96))
        }
        .padding()
    }
}

private struct ServerConnectionSheet: View {
    @Bindable var profileStore: ProfileStore
    @Environment(\.dismiss) private var dismiss
    @State private var customAddress: String = ""
    @State private var errorMessage: String?
    @State private var isConnecting = false

    var body: some View {
        NavigationStack {
            ZStack {
                MelloTVTheme.background
                    .ignoresSafeArea()

                ScrollView {
                    VStack(alignment: .leading, spacing: 24) {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("CURRENT SERVER")
                                .font(.caption.weight(.bold))
                                .foregroundStyle(MelloTVTheme.muted)

                            HStack(spacing: 12) {
                                Circle()
                                    .fill(profileStore.errorMessage == nil ? Color.green : Color.orange)
                                    .frame(width: 10, height: 10)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(profileStore.connectedServerName)
                                        .font(.headline)
                                        .foregroundStyle(MelloTVTheme.text)
                                    Text(profileStore.connectedServerAddress)
                                        .font(.caption)
                                        .foregroundStyle(MelloTVTheme.muted)
                                }
                                Spacer()
                            }
                            .padding(14)
                            .background(MelloTVTheme.cardBackground, in: RoundedRectangle(cornerRadius: 12))
                            .overlay(
                                RoundedRectangle(cornerRadius: 12)
                                    .stroke(MelloTVTheme.border, lineWidth: 1)
                            )
                        }

                        VStack(alignment: .leading, spacing: 10) {
                            HStack {
                                Text("DISCOVERED ON LOCAL NETWORK")
                                    .font(.caption.weight(.bold))
                                    .foregroundStyle(MelloTVTheme.muted)
                                Spacer()
                                Button {
                                    Task {
                                        await profileStore.scanForServers()
                                    }
                                } label: {
                                    HStack(spacing: 4) {
                                        if profileStore.isDiscoveringServer {
                                            ProgressView()
                                                .scaleEffect(0.75)
                                        }
                                        Text(profileStore.isDiscoveringServer ? "Scanning…" : "Rescan")
                                            .font(.caption.weight(.bold))
                                            .foregroundStyle(MelloTVTheme.accent)
                                    }
                                }
                                .disabled(profileStore.isDiscoveringServer)
                            }

                            if profileStore.discoveredServers.isEmpty {
                                Text(profileStore.isDiscoveringServer ? "Searching local network via UDP & Bonjour…" : "No other servers found on this Wi-Fi network.")
                                    .font(.subheadline)
                                    .foregroundStyle(MelloTVTheme.muted)
                                    .padding(.vertical, 8)
                            } else {
                                ForEach(profileStore.discoveredServers) { server in
                                    Button {
                                        Task {
                                            await profileStore.selectDiscoveredServer(server)
                                            dismiss()
                                        }
                                    } label: {
                                        HStack(spacing: 12) {
                                            PhosphorIcon("television", size: 20, weight: .bold)
                                                .foregroundStyle(MelloTVTheme.accent)
                                            VStack(alignment: .leading, spacing: 2) {
                                                Text(server.name)
                                                    .font(.subheadline.weight(.semibold))
                                                    .foregroundStyle(MelloTVTheme.text)
                                                Text(server.primaryURL.absoluteString)
                                                    .font(.caption)
                                                    .foregroundStyle(MelloTVTheme.muted)
                                            }
                                            Spacer()
                                            Text("Use")
                                                .font(.caption.weight(.bold))
                                                .foregroundStyle(MelloTVTheme.accent)
                                        }
                                        .padding(14)
                                        .background(MelloTVTheme.cardBackground, in: RoundedRectangle(cornerRadius: 12))
                                        .overlay(
                                            RoundedRectangle(cornerRadius: 12)
                                                .stroke(MelloTVTheme.border, lineWidth: 1)
                                        )
                                    }
                                }
                            }
                        }

                        VStack(alignment: .leading, spacing: 10) {
                            Text("CONNECT BY ADDRESS OR TAILSCALE IP")
                                .font(.caption.weight(.bold))
                                .foregroundStyle(MelloTVTheme.muted)

                            TextField("e.g. 192.168.1.50:8080 or 100.x.y.z:8080", text: $customAddress)
                                .textInputAutocapitalization(.never)
                                .autocorrectionDisabled()
                                .keyboardType(.URL)
                                .font(.subheadline)
                                .foregroundStyle(MelloTVTheme.text)
                                .padding(14)
                                .background(MelloTVTheme.cardBackground, in: RoundedRectangle(cornerRadius: 10))
                                .overlay(
                                    RoundedRectangle(cornerRadius: 10)
                                        .stroke(MelloTVTheme.border, lineWidth: 1)
                                )

                            if let errorMessage {
                                Text(errorMessage)
                                    .font(.caption)
                                    .foregroundStyle(.red)
                            }

                            Button {
                                Task {
                                    errorMessage = nil
                                    isConnecting = true
                                    do {
                                        try await profileStore.connectToCustomServer(customAddress)
                                        dismiss()
                                    } catch {
                                        errorMessage = error.localizedDescription
                                    }
                                    isConnecting = false
                                }
                            } label: {
                                HStack {
                                    Spacer()
                                    if isConnecting {
                                        ProgressView()
                                            .tint(.white)
                                    } else {
                                        Text("Connect to Server")
                                            .font(.headline)
                                            .foregroundStyle(.white)
                                    }
                                    Spacer()
                                }
                                .padding(.vertical, 14)
                                .background(MelloTVTheme.accent, in: RoundedRectangle(cornerRadius: 10))
                            }
                            .disabled(customAddress.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || isConnecting)
                        }
                    }
                    .padding(20)
                }
            }
            .navigationTitle("Server Connection")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        dismiss()
                    }
                    .foregroundStyle(MelloTVTheme.accent)
                }
            }
        }
    }
}
