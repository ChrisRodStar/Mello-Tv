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
    @ObservedObject var process: ServerProcessState
    let snapshot: ServerSnapshot?
    let actions: AppActions
    @State private var section = SettingsSection.general
    @Namespace private var segmentAnimation

    private var appVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0.5.0"
    }

    var body: some View {
        ScrollView(.vertical, showsIndicators: false) {
            VStack(spacing: 28) {
                // Centered Page Title & Subtitle
                VStack(spacing: 6) {
                    Text("Settings")
                        .font(.system(size: 32, weight: .bold))
                        .tracking(-0.8)
                        .foregroundStyle(MelloTVTheme.text)

                    Text("Preferences & Server Configuration")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                }
                .padding(.top, 20)

                // iOS-Style Segmented Pill Picker
                segmentedPicker
                    .frame(maxWidth: 480)

                // Section Content
                VStack(spacing: 20) {
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
                .frame(maxWidth: 620)
                .animation(.spring(response: 0.35, dampingFraction: 0.86), value: section)
                .padding(.bottom, 40)
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
                            .font(.system(size: 11, weight: .semibold))
                        Text(item.rawValue)
                            .font(.system(size: 13, weight: .medium))
                    }
                    .frame(maxWidth: .infinity)
                    .frame(height: 34)
                    .foregroundStyle(section == item ? MelloTVTheme.text : MelloTVTheme.muted)
                    .background {
                        if section == item {
                            Capsule()
                                .fill(MelloTVTheme.card)
                                .matchedGeometryEffect(id: "activeTab", in: segmentAnimation)
                                .overlay {
                                    Capsule().strokeBorder(MelloTVTheme.border, lineWidth: 1)
                                }
                                .shadow(color: .black.opacity(0.12), radius: 4, y: 2)
                        }
                    }
                    .contentShape(Capsule())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(4)
        .background(MelloTVTheme.sunken, in: Capsule())
        .overlay {
            Capsule().strokeBorder(MelloTVTheme.border, lineWidth: 1)
        }
    }

    // MARK: - General Section
    private var generalSection: some View {
        VStack(spacing: 16) {
            SettingsCard {
                // Appearance Selector (System / Light / Dark) matching iOS 1:1
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
                    .frame(width: 140)
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
                    .frame(width: 170)
                }

                SettingsDivider()

                SettingsItemRow(
                    icon: "power",
                    iconColor: Color(red: 0.35, green: 0.85, blue: 0.55),
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
                        .padding(.vertical, 4)
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
                // Server State Row
                HStack(spacing: 14) {
                    ZStack {
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(process.isRunning ? Color(red: 0.35, green: 0.85, blue: 0.55).opacity(0.18) : MelloTVTheme.sunken)
                            .frame(width: 36, height: 36)
                        Circle()
                            .fill(process.isRunning ? Color(red: 0.35, green: 0.85, blue: 0.55) : MelloTVTheme.mutedSubtle)
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

                    Spacer()

                    HStack(spacing: 8) {
                        if process.isRunning {
                            Button(action: actions.restartServer) {
                                Image(systemName: "arrow.clockwise")
                                    .font(.system(size: 12, weight: .semibold))
                                    .frame(width: 30, height: 30)
                            }
                            .buttonStyle(SecondaryGlassButtonStyle())
                            .help("Restart Server")
                        }

                        Button(process.isRunning ? "Stop" : "Start", action: actions.toggleServer)
                            .buttonStyle(PrimaryPillButtonStyle(destructive: process.isRunning))
                    }
                }
                .padding(.vertical, 4)

                SettingsDivider()

                SettingsItemRow(
                    icon: "doc.text.magnifyingglass",
                    iconColor: Color(red: 0.95, green: 0.6, blue: 0.2),
                    title: "Diagnostics & Logs",
                    subtitle: "View live backend request traces and errors"
                ) {
                    Button("Open Logs…", action: actions.openLogs)
                        .buttonStyle(SecondaryGlassButtonStyle())
                }

                SettingsDivider()

                SettingsItemRow(
                    icon: "folder",
                    iconColor: Color(red: 0.3, green: 0.7, blue: 0.9),
                    title: "Application Data",
                    subtitle: "Local database, caches, profiles, and state"
                ) {
                    Button("Show in Finder", action: actions.openData)
                        .buttonStyle(SecondaryGlassButtonStyle())
                }
            }

            FootnoteText(text: "The local server enables streaming, metadata scraping, and companion device synchronization.")
        }
    }

    // MARK: - Updates Section
    private var updatesSection: some View {
        VStack(spacing: 16) {
            SettingsCard {
                SettingsItemRow(
                    icon: "sparkles",
                    iconColor: Color(red: 0.9, green: 0.4, blue: 0.85),
                    title: "Mello TV Core",
                    subtitle: "Native macOS Desktop & Server bundle"
                ) {
                    Text("v\(appVersion)")
                        .font(.system(size: 13, weight: .medium, design: .monospaced))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 4)
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
                            iconColor: Color(red: 0.25, green: 0.75, blue: 0.8),
                            title: provider.name,
                            subtitle: provider.needsAttention ? "Needs configuration" : "Integrated provider module"
                        ) {
                            Text("v\(provider.version)")
                                .font(.system(size: 13, weight: .medium, design: .monospaced))
                                .foregroundStyle(provider.needsAttention ? .orange : MelloTVTheme.textSecondary)
                                .padding(.horizontal, 10)
                                .padding(.vertical, 4)
                                .background(MelloTVTheme.sunken, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
                                .overlay {
                                    RoundedRectangle(cornerRadius: 6, style: .continuous)
                                        .strokeBorder(MelloTVTheme.border, lineWidth: 1)
                                }
                        }
                    }
                }
            }

            FootnoteText(text: "Provider extensions and server updates are packaged and distributed with Mello TV releases.")
        }
    }

    // MARK: - About Section
    private var aboutSection: some View {
        VStack(spacing: 20) {
            // App Brand Banner
            VStack(spacing: 12) {
                if let icon = NSApp.applicationIconImage {
                    Image(nsImage: icon)
                        .resizable()
                        .scaledToFit()
                        .frame(width: 72, height: 72)
                        .shadow(color: .black.opacity(0.2), radius: 10, y: 5)
                }

                VStack(spacing: 4) {
                    Text("Mello TV")
                        .font(.system(size: 24, weight: .bold))
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
            .padding(.vertical, 8)

            // Links Card
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

                // Buy Me a Coffee Row - Matching iOS aesthetic
                SettingsItemRow(
                    icon: "cup.and.saucer.fill",
                    iconColor: Color(red: 1.0, green: 0.84, blue: 0.1),
                    title: "Buy Me a Coffee",
                    subtitle: "Support independent development of Mello TV"
                ) {
                    Link(destination: AppLinks.buyMeACoffeeURL) {
                        HStack(spacing: 6) {
                            Image(systemName: "cup.and.saucer.fill")
                                .font(.system(size: 12, weight: .semibold))
                                .foregroundStyle(Color(red: 1.0, green: 0.84, blue: 0.1))
                            Text("Support")
                                .font(.system(size: 13, weight: .semibold))
                                .foregroundStyle(Color(red: 1.0, green: 0.84, blue: 0.1))
                            Image(systemName: "arrow.up.forward")
                                .font(.system(size: 10, weight: .bold))
                                .foregroundStyle(Color(red: 1.0, green: 0.84, blue: 0.1).opacity(0.8))
                        }
                        .padding(.horizontal, 14)
                        .padding(.vertical, 7)
                        .background(Color(red: 1.0, green: 0.84, blue: 0.1).opacity(0.12), in: Capsule())
                        .overlay {
                            Capsule().strokeBorder(Color(red: 1.0, green: 0.84, blue: 0.1).opacity(0.25), lineWidth: 1)
                        }
                    }
                    .buttonStyle(.plain)
                }
            }

            FootnoteText(text: "Crafted with care by Chris Sinclair.")
        }
    }
}

// MARK: - Reusable UI Components

private struct SettingsCard<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        VStack(spacing: 0) {
            content
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 14)
        .background(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .fill(MelloTVTheme.card)
        )
        .overlay {
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .strokeBorder(MelloTVTheme.border, lineWidth: 1)
        }
        .shadow(color: Color.black.opacity(0.08), radius: 8, y: 3)
    }
}

private struct SettingsItemRow<Control: View>: View {
    let icon: String
    var iconColor: Color = .white
    let title: String
    var subtitle: String? = nil
    @ViewBuilder var control: Control

    var body: some View {
        HStack(spacing: 14) {
            ZStack {
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .fill(iconColor.opacity(0.16))
                    .frame(width: 32, height: 32)
                Image(systemName: icon)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(iconColor)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(MelloTVTheme.text)

                if let subtitle {
                    Text(subtitle)
                        .font(.system(size: 11, weight: .regular))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }

            Spacer(minLength: 16)

            control
        }
        .padding(.vertical, 8)
        .frame(minHeight: 44)
    }
}

private struct SettingsDivider: View {
    var body: some View {
        Divider()
            .overlay(MelloTVTheme.border)
            .padding(.leading, 46)
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

// MARK: - Button Styles

private struct PrimaryPillButtonStyle: ButtonStyle {
    var destructive: Bool = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 12, weight: .semibold))
            .padding(.horizontal, 16)
            .padding(.vertical, 7)
            .foregroundStyle(destructive ? MelloTVTheme.text : Color.black)
            .background(
                destructive ? MelloTVTheme.sunken : Color(white: 0.94),
                in: Capsule()
            )
            .overlay {
                Capsule().strokeBorder(
                    destructive ? MelloTVTheme.border : Color.clear,
                    lineWidth: 1
                )
            }
            .opacity(configuration.isPressed ? 0.7 : 1.0)
            .scaleEffect(configuration.isPressed ? 0.97 : 1.0)
    }
}

private struct SecondaryGlassButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 12, weight: .medium))
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .foregroundStyle(MelloTVTheme.text)
            .background(MelloTVTheme.cardHover, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .strokeBorder(MelloTVTheme.border, lineWidth: 1)
            }
            .opacity(configuration.isPressed ? 0.7 : 1.0)
            .scaleEffect(configuration.isPressed ? 0.97 : 1.0)
    }
}
