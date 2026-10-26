import AppKit
import SwiftUI

struct DesktopSidebarView: View {
    @ObservedObject var navigation: DesktopNavigation
    @ObservedObject var model: ServerStatusStore
    @ObservedObject var process: ServerProcessState
    let actions: AppActions
    private var expanded: Bool { navigation.isSidebarExpanded }

    var body: some View {
        VStack(spacing: 0) {
            Button { navigation.selection = .overview } label: {
                HStack(spacing: 9) {
                    if let icon = NSApp.applicationIconImage {
                        Image(nsImage: icon).resizable().scaledToFit().frame(width: 30, height: 30)
                            .accessibilityHidden(true)
                    }
                    if expanded {
                        Text("Mello TV").font(.system(size: 14, weight: .semibold))
                        Spacer(minLength: 0)
                    }
                }
                .frame(maxWidth: .infinity, alignment: expanded ? .leading : .center)
                .frame(height: 36)
                .padding(.horizontal, expanded ? 8 : 0)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Mello TV overview")
            .padding(.bottom, 22)

            VStack(spacing: 3) {
                ForEach(DesktopDestination.allCases) { destination in
                    SidebarNavigationButton(title: destination.rawValue, symbol: destination.symbol,
                                            expanded: expanded, selected: navigation.selection == destination) {
                        navigation.selection = destination
                    }
                    .keyboardShortcut(destination.shortcut, modifiers: .command)
                }
            }
            Spacer(minLength: 24)
            HStack(spacing: 8) {
                Circle().fill(process.isRunning && model.isReachable ? Color.green : process.isRunning ? .orange : .gray)
                    .frame(width: 6, height: 6)
                    .accessibilityHidden(true)
                if expanded {
                    Text(process.isRunning && model.isReachable ? "Server online" : process.isRunning ? "Connecting…" : "Server stopped")
                        .font(.system(size: 11)).foregroundStyle(.secondary)
                    Spacer(minLength: 0)
                }
            }
            .frame(maxWidth: .infinity)
            .padding(.horizontal, expanded ? 12 : 0)
            .padding(.bottom, 16)
            Divider().overlay(MelloTVTheme.hairline).padding(.horizontal, 8).padding(.bottom, 10)
            VStack(spacing: 3) {
                SidebarNavigationButton(title: "Logs", symbol: "doc.text", expanded: expanded, action: actions.openLogs)
                SidebarNavigationButton(title: "Welcome guide", symbol: "questionmark.circle", expanded: expanded, action: actions.showWelcome)
                SidebarNavigationButton(title: "Support Mello TV", symbol: "heart", expanded: expanded) {
                    NSWorkspace.shared.open(AppLinks.buyMeACoffeeURL)
                }
            }
        }
        .padding(.top, 14)
        .padding(.horizontal, 8)
        .padding(.bottom, 12)
        .background(MelloTVTheme.workspace)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Main navigation")
    }
}

private struct SidebarNavigationButton: View {
    let title: String
    let symbol: String
    let expanded: Bool
    var selected = false
    let action: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 11) {
                Image(systemName: symbol).font(.system(size: 15))
                    .frame(width: 20, height: 20).accessibilityHidden(true)
                if expanded {
                    Text(title).font(.system(size: 13, weight: selected ? .medium : .regular)).lineLimit(1)
                    Spacer(minLength: 0)
                }
            }
            .frame(maxWidth: .infinity, alignment: expanded ? .leading : .center)
            .frame(height: 36)
            .padding(.horizontal, expanded ? 10 : 0)
            .contentShape(Rectangle())
            .background(selected ? MelloTVTheme.selection : hovered ? MelloTVTheme.quietFill : .clear,
                        in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        }
        .buttonStyle(.plain)
        .foregroundStyle(selected ? Color.white : Color.white.opacity(0.75))
        .onHover { hovered = $0 }
        .help(title)
        .accessibilityLabel(title)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}
