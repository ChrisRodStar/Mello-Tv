import SwiftUI
import MelloKit

/// 10-foot tvOS Settings view displaying profile selector, server connection status,
/// appearance preferences, and application version info.
public struct SettingsView: View {
    @ObservedObject var serverAddress = ServerAddress.shared
    @ObservedObject var profileStore = ProfileStore.shared

    @AppStorage("mello_appearance_mode") private var appearanceModeRaw: String = AppearanceMode.dark.rawValue

    public init() {}

    private var currentAppearance: AppearanceMode {
        AppearanceMode(rawValue: appearanceModeRaw) ?? .dark
    }

    private var appVersion: String {
        Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "0.3.0"
    }

    private var appBuild: String {
        Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "5"
    }

    public var body: some View {
        ScrollView(.vertical, showsIndicators: false) {
            VStack(alignment: .leading, spacing: 48) {
                // Header
                Text("Settings")
                    .font(.system(size: 44, weight: .bold))
                    .foregroundColor(MelloTVTheme.text)

                // 1. Profile Selector Section
                profilesSection

                // 2. Server Connection Section
                serverSection

                // 3. Appearance Mode Section
                appearanceSection

                // 4. About App Section
                aboutSection
            }
            .padding(.horizontal, 64)
            .padding(.top, 32)
            .padding(.bottom, 64)
        }
        .background(MelloTVTheme.background.ignoresSafeArea())
        .task {
            await profileStore.loadProfiles()
        }
    }

    // MARK: - Profile Selector Section

    private var profilesSection: some View {
        VStack(alignment: .leading, spacing: 20) {
            VStack(alignment: .leading, spacing: 4) {
                Text("Profiles")
                    .font(.title2.weight(.bold))
                    .foregroundColor(MelloTVTheme.text)
                Text("Switch between household viewing profiles to personalize your recommendations.")
                    .font(.callout)
                    .foregroundColor(MelloTVTheme.muted)
            }

            if profileStore.isLoading && profileStore.profiles.isEmpty {
                ProgressView()
                    .padding(.vertical, 20)
            } else if !profileStore.profiles.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 24) {
                        ForEach(profileStore.profiles) { profile in
                            profileCard(profile)
                        }
                    }
                    .padding(.vertical, 12)
                }
            } else {
                Text("No profiles configured on this server.")
                    .font(.callout)
                    .foregroundColor(MelloTVTheme.muted)
            }
        }
    }

    private func profileCard(_ profile: Profile) -> some View {
        let isActive = profile.id == profileStore.activeProfileID
        let color = ProfileColor.color(for: profile.color)

        return Button(action: {
            selectProfile(profile)
        }) {
            VStack(spacing: 14) {
                // Avatar Circle
                ZStack {
                    Circle()
                        .fill(color)
                        .frame(width: 100, height: 100)
                        .shadow(color: color.opacity(isActive ? 0.6 : 0.2), radius: isActive ? 16 : 6)

                    Text(String(profile.name.prefix(1)).uppercased())
                        .font(.system(size: 42, weight: .bold))
                        .foregroundColor(.white)

                    if isActive {
                        VStack {
                            Spacer()
                            HStack {
                                Spacer()
                                Image(systemName: "checkmark.circle.fill")
                                    .font(.system(size: 24))
                                    .foregroundColor(.white)
                                    .background(Circle().fill(Color.black.opacity(0.8)))
                            }
                        }
                        .frame(width: 100, height: 100)
                    }
                }

                // Profile Name
                Text(profile.name)
                    .font(.headline.weight(isActive ? .bold : .medium))
                    .foregroundColor(isActive ? .white : MelloTVTheme.textSecondary)
                    .lineLimit(1)

                // Active badge
                if isActive {
                    Text("ACTIVE")
                        .font(.system(size: 11, weight: .black))
                        .tracking(1.2)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 3)
                        .background(MelloTVTheme.accent.opacity(0.3))
                        .clipShape(Capsule())
                        .foregroundStyle(MelloTVTheme.accent)
                } else {
                    Text("Switch")
                        .font(.caption)
                        .foregroundColor(.clear)
                }
            }
            .frame(width: 180, height: 210)
            .padding(16)
            .background(Color(white: isActive ? 0.20 : 0.12))
            .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .stroke(isActive ? MelloTVTheme.accent : Color.clear, lineWidth: 3)
            )
        }
        .buttonStyle(.plain)
    }

    private func selectProfile(_ profile: Profile) {
        profileStore.selectProfile(profile)
    }

    // MARK: - Server Connection Section

    private var serverSection: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text("Server Connection")
                .font(.title2.weight(.bold))
                .foregroundColor(MelloTVTheme.text)

            HStack(spacing: 32) {
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 8) {
                        Circle()
                            .fill(serverAddress.isConnected ? Color.green : Color.orange)
                            .frame(width: 10, height: 10)
                        Text(serverAddress.isConnected ? "CONNECTED" : "CONNECTING")
                            .font(.caption2.weight(.black))
                            .tracking(1.2)
                            .foregroundColor(serverAddress.isConnected ? .green : .orange)
                    }

                    Text(serverAddress.serverName)
                        .font(.title3.weight(.semibold))
                        .foregroundColor(.primary)

                    Text(serverAddress.activeBaseURL?.absoluteString ?? "Not connected")
                        .font(.callout)
                        .foregroundColor(MelloTVTheme.muted)
                }

                Spacer()

                Button("Disconnect", role: .destructive) {
                    serverAddress.disconnect()
                }
                .buttonStyle(.bordered)
            }
            .padding(28)
            .background(Color(white: 0.14))
            .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        }
    }

    // MARK: - Appearance Section

    private var appearanceSection: some View {
        VStack(alignment: .leading, spacing: 20) {
            VStack(alignment: .leading, spacing: 4) {
                Text("Appearance")
                    .font(.title2.weight(.bold))
                    .foregroundColor(MelloTVTheme.text)
                Text("Choose between system, light, or OLED dark color schemes.")
                    .font(.callout)
                    .foregroundColor(MelloTVTheme.muted)
            }

            HStack(spacing: 20) {
                ForEach(AppearanceMode.allCases) { mode in
                    let isSelected = appearanceModeRaw == mode.rawValue
                    Button(action: {
                        appearanceModeRaw = mode.rawValue
                    }) {
                        HStack(spacing: 12) {
                            Image(systemName: mode.systemImageName)
                                .font(.headline)
                            Text(mode.rawValue)
                                .font(.headline.weight(isSelected ? .bold : .medium))
                        }
                        .padding(.horizontal, 28)
                        .padding(.vertical, 16)
                        .frame(minWidth: 180)
                        .background(Color(white: isSelected ? 0.25 : 0.14))
                        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 14, style: .continuous)
                                .stroke(isSelected ? MelloTVTheme.accent : Color.clear, lineWidth: 3)
                        )
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    // MARK: - About Section

    private var aboutSection: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text("About Mello TV")
                .font(.title2.weight(.bold))
                .foregroundColor(MelloTVTheme.text)

            VStack(alignment: .leading, spacing: 16) {
                infoRow(label: "Version", value: "\(appVersion) (Build \(appBuild))")
                Divider().background(Color.white.opacity(0.1))
                infoRow(label: "Platform", value: "Apple TV (tvOS 17+)")
                Divider().background(Color.white.opacity(0.1))
                infoRow(label: "Engine", value: "Native Swift / SwiftUI & AVPlayer")
            }
            .padding(28)
            .background(Color(white: 0.14))
            .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        }
    }

    private func infoRow(label: String, value: String) -> some View {
        HStack {
            Text(label)
                .font(.headline)
                .foregroundColor(MelloTVTheme.muted)
            Spacer()
            Text(value)
                .font(.headline.weight(.semibold))
                .foregroundColor(.white)
        }
    }
}
