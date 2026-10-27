import AppKit
import SwiftUI

private enum SettingsSection: String, CaseIterable, Identifiable {
    case general = "General"
    case server = "Server"
    case updates = "Updates"
    case about = "About"
    var id: Self { self }

    var icon: String {
        switch self {
        case .general: return "slider.horizontal.3"
        case .server: return "server.rack"
        case .updates: return "arrow.triangle.2.circlepath"
        case .about: return "info.circle"
        }
    }
}

struct SettingsView: View {
    @ObservedObject var preferences: AppPreferences
    @ObservedObject var updater: AppUpdateService
    @ObservedObject var process: ServerProcessState
    let snapshot: ServerSnapshot?
    let actions: AppActions
    @State private var section = SettingsSection.general
    @Namespace private var segmentAnimation
    @Environment(\.colorScheme) private var colorScheme

    private var appVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "MelloTVReleaseVersion") as? String
            ?? Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
            ?? "0.1.0-alpha.2"
    }

    var body: some View {
        ScrollView(.vertical, showsIndicators: false) {
            VStack(spacing: 22) {
                // Fixed-position Segmented Capsule Control (never jumps between tabs)
                segmentedPicker
                    .frame(maxWidth: 500)
                    .padding(.top, 28)

                // Section Content anchored directly beneath the fixed tabs
                VStack(spacing: 16) {
                    switch section {
                    case .general:
                        generalSection
                    case .server:
                        serverSection
                    case .updates:
                        updatesSection
                    case .about:
                        aboutSection
                    }
                }
                .frame(maxWidth: 740)
                .frame(minHeight: 280, alignment: .top)
                .animation(.spring(response: 0.35, dampingFraction: 0.86), value: section)
                .padding(.bottom, 36)
            }
            .frame(maxWidth: .infinity)
            .padding(.horizontal, 32)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: - Segmented Picker
    private var segmentedPicker: some View {
        HStack(spacing: 4) {
            ForEach(SettingsSection.allCases) { item in
                Button {
                    withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) {
                        section = item
                    }
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: item.icon)
                            .font(.system(size: 12, weight: .semibold))
                        Text(item.rawValue)
                            .font(.system(size: 13, weight: .semibold))
                    }
                    .frame(maxWidth: .infinity)
                    .frame(height: 36)
                    .foregroundStyle(section == item ? MelloTVTheme.text : MelloTVTheme.muted)
                    .background {
                        if section == item {
                            Capsule()
                                .fill(MelloTVTheme.card)
                                .matchedGeometryEffect(id: "activeTab", in: segmentAnimation)
                                .overlay {
                                    Capsule().strokeBorder(MelloTVTheme.border, lineWidth: 1)
                                }
                                .shadow(color: .black.opacity(colorScheme == .dark ? 0.3 : 0.08), radius: 4, y: 2)
                        }
                    }
                    .contentShape(Capsule())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(5)
        .background(MelloTVTheme.sunken, in: Capsule())
        .overlay {
            Capsule().strokeBorder(MelloTVTheme.border, lineWidth: 1)
        }
    }

    // MARK: - General Section
    private var generalSection: some View {
        VStack(spacing: 16) {
            SettingsCard {
                SettingsItemRow(
                    icon: preferences.appearance.iconName,
                    iconColor: MelloTVTheme.accent,
                    title: "Appearance",
                    subtitle: "Choose between Light, Dark, or System matching"
                ) {
                    Picker("", selection: Binding(
                        get: { preferences.appearance },
                        set: { preferences.setAppearance($0) }
                    )) {
                        ForEach(AppearanceMode.allCases) { mode in
                            Label(mode.rawValue, systemImage: mode.iconName).tag(mode)
                        }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .frame(width: 160, alignment: .trailing)
                }

                SettingsDivider()

                SettingsItemRow(
                    icon: "macwindow",
                    iconColor: Color(red: 0.35, green: 0.55, blue: 0.95),
                    title: "Presentation",
                    subtitle: "Choose where Mello TV appears"
                ) {
                    Picker("", selection: Binding(
                        get: { preferences.presentation },
                        set: { preferences.setPresentation($0) }
                    )) {
                        ForEach(AppPresentation.allCases) { mode in
                            Text(mode.title).tag(mode)
                        }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)
                    .frame(width: 160, alignment: .trailing)
                }

                SettingsDivider()

                SettingsItemRow(
                    icon: "power",
                    iconColor: Color(red: 0.25, green: 0.78, blue: 0.45),
                    title: "Launch at Login",
                    subtitle: "Start Mello TV automatically when you sign in"
                ) {
                    Toggle("", isOn: Binding(
                        get: { preferences.launchAtLogin },
                        set: { preferences.setLaunchAtLogin($0) }
                    ))
                    .toggleStyle(.switch)
                    .labelsHidden()
                }

                SettingsDivider()

                SettingsItemRow(
                    icon: "info.circle",
                    iconColor: MelloTVTheme.muted,
                    title: "App Version",
                    subtitle: "Current installed build"
                ) {
                    Text(appVersion)
                        .font(.system(size: 13, weight: .medium, design: .monospaced))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 5)
                        .background(MelloTVTheme.sunken, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
                        .overlay {
                            RoundedRectangle(cornerRadius: 6, style: .continuous)
                                .strokeBorder(MelloTVTheme.border, lineWidth: 1)
                        }
                }
            }

            if let error = preferences.presentationError {
                HStack(spacing: 8) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                    Text(error)
                        .font(.system(size: 12))
                        .foregroundStyle(.orange)
                }
                .padding(.horizontal, 8)
                .frame(maxWidth: .infinity, alignment: .leading)
            }

            FootnoteText(text: "Closing the main window keeps your server active. Reopen it anytime from the menu bar or Dock.")
        }
    }

    // MARK: - Server Section
    private var serverSection: some View {
        VStack(spacing: 16) {
            SettingsCard {
                HStack(spacing: 16) {
                    ZStack {
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(process.isRunning ? Color(red: 0.25, green: 0.78, blue: 0.45).opacity(0.18) : MelloTVTheme.sunken)
                            .frame(width: 36, height: 36)
                        Circle()
                            .fill(process.isRunning ? Color(red: 0.25, green: 0.78, blue: 0.45) : MelloTVTheme.mutedSubtle)
                            .frame(width: 10, height: 10)
                    }

                    VStack(alignment: .leading, spacing: 3) {
                        Text("Mello TV Server")
                            .font(.system(size: 14, weight: .semibold))
                            .foregroundStyle(MelloTVTheme.text)
                        Text(process.isRunning ? "Online and broadcasting on this Mac" : "Server is currently stopped")
                            .font(.system(size: 12))
                            .foregroundStyle(MelloTVTheme.textSecondary)
                    }

                    Spacer(minLength: 20)

                    HStack(spacing: 10) {
                        if process.isRunning {
                            Button(action: actions.restartServer) {
                                Label("Restart", systemImage: "arrow.clockwise")
                                    .font(.system(size: 12, weight: .medium))
                            }
                            .buttonStyle(ServerSecondaryButtonStyle())
                        }

                        Button(action: actions.toggleServer) {
                            Label(process.isRunning ? "Stop" : "Start", systemImage: "power")
                                .font(.system(size: 12, weight: .semibold))
                        }
                        .buttonStyle(ServerPrimaryButtonStyle(isStop: process.isRunning))
                    }
                    .frame(alignment: .trailing)
                }
                .padding(.vertical, 6)
                .frame(minHeight: 48)

                SettingsDivider()

                SettingsItemRow(
                    icon: "doc.text.magnifyingglass",
                    iconColor: Color(red: 0.95, green: 0.58, blue: 0.15),
                    title: "Diagnostics & Logs",
                    subtitle: "View live backend request traces and errors"
                ) {
                    Button("Open Logs…", action: actions.openLogs)
                        .buttonStyle(SecondaryGlassButtonStyle())
                }

                SettingsDivider()

                SettingsItemRow(
                    icon: "folder",
                    iconColor: Color(red: 0.2, green: 0.65, blue: 0.88),
                    title: "Application Data",
                    subtitle: "Local database, caches, profiles, and state"
                ) {
                    Button("Show in Finder", action: actions.openData)
                        .buttonStyle(SecondaryGlassButtonStyle())
                }
            }

            CatalogCredentialSettings(onRestart: actions.restartServer)

            FootnoteText(text: "The local server enables streaming, catalog updates, and companion device synchronization.")
        }
    }

    // MARK: - Updates Section
    private var updatesSection: some View {
        VStack(spacing: 16) {
            SettingsCard {
                SettingsItemRow(
                    icon: "arrow.triangle.2.circlepath",
                    iconColor: MelloTVTheme.accent,
                    title: "Software Updates",
                    subtitle: updater.statusMessage
                ) {
                    Button("Check for Updates…", action: updater.checkForUpdates)
                        .disabled(!updater.canCheckForUpdates)
                        .controlSize(.regular)
                }

                SettingsDivider()

                SettingsItemRow(
                    icon: "clock",
                    iconColor: MelloTVTheme.muted,
                    title: "Check Automatically",
                    subtitle: "Look for new versions in the background"
                ) {
                    Toggle("Check automatically", isOn: Binding(
                        get: { updater.automaticallyChecksForUpdates },
                        set: { updater.setAutomaticallyChecksForUpdates($0) }
                    ))
                    .toggleStyle(.switch)
                    .labelsHidden()
                }

                SettingsDivider()

                SettingsItemRow(
                    icon: "arrow.down.circle",
                    iconColor: MelloTVTheme.muted,
                    title: "Update Automatically",
                    subtitle: "Download and install when streaming is idle"
                ) {
                    Toggle("Update automatically", isOn: Binding(
                        get: { updater.automaticallyDownloadsUpdates },
                        set: { updater.setAutomaticallyDownloadsUpdates($0) }
                    ))
                    .toggleStyle(.switch)
                    .labelsHidden()
                    .disabled(!updater.automaticallyChecksForUpdates)
                }
            }

            SettingsCard {
                SettingsItemRow(
                    icon: "sparkles",
                    iconColor: Color(red: 0.85, green: 0.35, blue: 0.8),
                    title: "Mello TV Core",
                    subtitle: "Native macOS Desktop & Server bundle"
                ) {
                    Text("v\(appVersion)")
                        .font(.system(size: 13, weight: .medium, design: .monospaced))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 5)
                        .background(MelloTVTheme.sunken, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
                        .overlay {
                            RoundedRectangle(cornerRadius: 6, style: .continuous)
                                .strokeBorder(MelloTVTheme.border, lineWidth: 1)
                        }
                }

                if let providers = snapshot?.media.providers, !providers.isEmpty {
                    ForEach(providers) { provider in
                        SettingsDivider()
                        SettingsItemRow(
                            icon: "puzzlepiece.extension",
                            iconColor: Color(red: 0.25, green: 0.72, blue: 0.75),
                            title: provider.name,
                            subtitle: provider.needsAttention ? "Needs configuration" : "Integrated provider module"
                        ) {
                            Text("v\(provider.version)")
                                .font(.system(size: 13, weight: .medium, design: .monospaced))
                                .foregroundStyle(provider.needsAttention ? .orange : MelloTVTheme.textSecondary)
                                .padding(.horizontal, 10)
                                .padding(.vertical, 5)
                                .background(MelloTVTheme.sunken, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
                                .overlay {
                                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                                        .strokeBorder(MelloTVTheme.border, lineWidth: 1)
                                }
                        }
                    }
                }
            }

            FootnoteText(text: "Updates include the Mac app, server, and bundled providers. Installation waits until streaming is idle, then restarts Mello TV. Your profiles and library are kept.")
        }
    }

    // MARK: - About Section
    private var aboutSection: some View {
        VStack(spacing: 20) {
            VStack(spacing: 12) {
                if let icon = NSApp.applicationIconImage {
                    Image(nsImage: icon)
                        .resizable()
                        .scaledToFit()
                        .frame(width: 72, height: 72)
                        .shadow(color: .black.opacity(colorScheme == .dark ? 0.35 : 0.08), radius: 8, y: 4)
                }

                VStack(spacing: 4) {
                    Text("Mello TV")
                        .font(.system(size: 22, weight: .bold))
                        .tracking(-0.5)
                        .foregroundStyle(MelloTVTheme.text)

                    Text("Version \(appVersion)")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                }

                Text("A quiet, ad-free cinema experience for all your movies and TV shows.")
                    .font(.system(size: 13))
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 380)
            }
            .padding(.vertical, 4)

            SettingsCard {
                SettingsItemRow(
                    icon: "safari",
                    iconColor: MelloTVTheme.accent,
                    title: "Official Website",
                    subtitle: "Documentation and downloads"
                ) {
                    Link(destination: AppLinks.websiteURL) {
                        HStack(spacing: 4) {
                            Text("Visit")
                            Image(systemName: "arrow.up.forward")
                                .font(.system(size: 10, weight: .bold))
                        }
                    }
                    .buttonStyle(SecondaryGlassButtonStyle())
                }

                SettingsDivider()

                SettingsItemRow(
                    icon: "chevron.left.forwardslash.chevron.right",
                    iconColor: Color(red: 0.65, green: 0.45, blue: 0.95),
                    title: "Source Code",
                    subtitle: "GitHub repository and contributions"
                ) {
                    Link(destination: AppLinks.sourceCodeURL) {
                        HStack(spacing: 4) {
                            Text("GitHub")
                            Image(systemName: "arrow.up.forward")
                                .font(.system(size: 10, weight: .bold))
                        }
                    }
                    .buttonStyle(SecondaryGlassButtonStyle())
                }

                SettingsDivider()

                SettingsItemRow(
                    icon: "cup.and.saucer.fill",
                    iconColor: Color(red: 0.95, green: 0.75, blue: 0.05),
                    title: "Buy Me a Coffee",
                    subtitle: "Support independent development of Mello TV"
                ) {
                    Link(destination: AppLinks.buyMeACoffeeURL) {
                        HStack(spacing: 6) {
                            Image(systemName: "cup.and.saucer.fill")
                                .font(.system(size: 12, weight: .semibold))
                                .foregroundStyle(Color(red: 0.85, green: 0.65, blue: 0.0))
                            Text("Support")
                                .font(.system(size: 13, weight: .semibold))
                                .foregroundStyle(Color(red: 0.85, green: 0.65, blue: 0.0))
                            Image(systemName: "arrow.up.forward")
                                .font(.system(size: 10, weight: .bold))
                                .foregroundStyle(Color(red: 0.85, green: 0.65, blue: 0.0).opacity(0.8))
                        }
                        .padding(.horizontal, 14)
                        .padding(.vertical, 7)
                        .background(Color(red: 0.95, green: 0.75, blue: 0.05).opacity(colorScheme == .dark ? 0.15 : 0.10), in: Capsule())
                        .overlay {
                            Capsule().strokeBorder(Color(red: 0.95, green: 0.75, blue: 0.05).opacity(0.3), lineWidth: 1)
                        }
                    }
                    .buttonStyle(.plain)
                }
            }

            VStack(spacing: 8) {
                Link(destination: URL(string: "https://www.themoviedb.org")!) {
                    if let url = Bundle.main.resourceURL?.appendingPathComponent("web/attribution/tmdb-logo.svg"),
                       let logo = NSImage(contentsOf: url) {
                        Image(nsImage: logo).resizable().scaledToFit().frame(width: 68, height: 49)
                            .accessibilityLabel("TMDB")
                    } else {
                        Text("TMDB")
                    }
                }
                Text("This product uses the TMDB API but is not endorsed or certified by TMDB.")
                    .font(.caption)
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .multilineTextAlignment(.center)
            }

            FootnoteText(text: "Crafted with care by Chris Sinclair.")
        }
    }
}

// MARK: - Reusable UI Components

private struct SettingsCard<Content: View>: View {
    @ViewBuilder var content: Content
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        VStack(spacing: 0) {
            content
        }
        .padding(.horizontal, 24)
        .padding(.vertical, 12)
        .background(
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .fill(MelloTVTheme.card)
        )
        .overlay {
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .strokeBorder(MelloTVTheme.border, lineWidth: 1)
        }
        .shadow(color: Color.black.opacity(colorScheme == .dark ? 0.3 : 0.06), radius: 10, y: 3)
    }
}

private struct SettingsItemRow<Control: View>: View {
    let icon: String
    var iconColor: Color = .white
    let title: String
    var subtitle: String? = nil
    @ViewBuilder var control: Control

    var body: some View {
        HStack(spacing: 16) {
            ZStack {
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(iconColor.opacity(0.16))
                    .frame(width: 36, height: 36)
                Image(systemName: icon)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(iconColor)
            }

            VStack(alignment: .leading, spacing: 3) {
                Text(title)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(MelloTVTheme.text)

                if let subtitle {
                    Text(subtitle)
                        .font(.system(size: 12, weight: .regular))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }

            Spacer(minLength: 20)

            control
                .frame(alignment: .trailing)
        }
        .padding(.vertical, 8)
        .frame(minHeight: 48)
    }
}

private struct SettingsDivider: View {
    var body: some View {
        Divider()
            .overlay(MelloTVTheme.border)
            .padding(.leading, 52)
    }
}

private struct FootnoteText: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.system(size: 12))
            .foregroundStyle(MelloTVTheme.muted)
            .multilineTextAlignment(.center)
            .frame(maxWidth: .infinity)
            .padding(.horizontal, 16)
    }
}

// MARK: - Server Action Buttons

private struct ServerPrimaryButtonStyle: ButtonStyle {
    var isStop: Bool
    @Environment(\.colorScheme) private var colorScheme

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .padding(.horizontal, 16)
            .frame(height: 32)
            .foregroundStyle(isStop ? Color(red: 0.95, green: 0.25, blue: 0.25) : Color.white)
            .background(
                isStop
                    ? Color(red: 0.95, green: 0.25, blue: 0.25).opacity(colorScheme == .dark ? 0.18 : 0.12)
                    : MelloTVTheme.accent,
                in: Capsule()
            )
            .overlay {
                Capsule().strokeBorder(
                    isStop ? Color(red: 0.95, green: 0.25, blue: 0.25).opacity(0.3) : Color.clear,
                    lineWidth: 1
                )
            }
            .opacity(configuration.isPressed ? 0.75 : 1.0)
            .scaleEffect(configuration.isPressed ? 0.97 : 1.0)
    }
}

private struct ServerSecondaryButtonStyle: ButtonStyle {
    @Environment(\.colorScheme) private var colorScheme

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .padding(.horizontal, 16)
            .frame(height: 32)
            .foregroundStyle(MelloTVTheme.text)
            .background(MelloTVTheme.sunken, in: Capsule())
            .overlay {
                Capsule().strokeBorder(MelloTVTheme.border, lineWidth: 1)
            }
            .opacity(configuration.isPressed ? 0.75 : 1.0)
            .scaleEffect(configuration.isPressed ? 0.97 : 1.0)
    }
}

private struct SecondaryGlassButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 12, weight: .medium))
            .padding(.horizontal, 14)
            .padding(.vertical, 7)
            .foregroundStyle(MelloTVTheme.text)
            .background(MelloTVTheme.sunken, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .strokeBorder(MelloTVTheme.border, lineWidth: 1)
            }
            .opacity(configuration.isPressed ? 0.7 : 1.0)
            .scaleEffect(configuration.isPressed ? 0.97 : 1.0)
    }
}
