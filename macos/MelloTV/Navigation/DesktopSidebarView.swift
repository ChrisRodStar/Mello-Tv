import AppKit
import SwiftUI

struct DesktopSidebarView: View {
    @ObservedObject var navigation: DesktopNavigation
    let actions: AppActions
    private var expanded: Bool { navigation.isSidebarExpanded }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            brand
                .padding(.bottom, 20)

            VStack(alignment: .leading, spacing: 20) {
                sidebarSection("Server") {
                    destinationButton(.overview)
                    destinationButton(.providers)
                    destinationButton(.network)
                }
            }

            Spacer(minLength: 24)

            VStack(spacing: 4) {
                destinationButton(.settings)
                    .padding(.bottom, 8)
                SidebarRowButton(
                    title: "Welcome guide",
                    symbol: "book.closed",
                    expanded: expanded,
                    action: actions.showWelcome
                )
                SidebarRowButton(
                    title: "Support",
                    symbol: "heart.fill",
                    expanded: expanded,
                    symbolColor: Color(red: 0.94, green: 0.22, blue: 0.29)
                ) {
                    NSWorkspace.shared.open(AppLinks.buyMeACoffeeURL)
                }
                .help("Support Mello TV on Buy Me a Coffee")
            }
        }
        .padding(.horizontal, 10)
        .padding(.top, 4)
        .padding(.bottom, 8)
        .background(Color.clear)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Main navigation")
    }

    private var brand: some View {
        Button { navigation.selection = .overview } label: {
            HStack(spacing: 10) {
                if let icon = NSApp.applicationIconImage {
                    Image(nsImage: icon)
                        .resizable()
                        .scaledToFit()
                        .frame(width: 30, height: 30)
                        .accessibilityHidden(true)
                }
                if expanded {
                    Text("Mello TV")
                        .font(.system(size: 15, weight: .semibold))
                        .tracking(-0.3)
                        .foregroundStyle(MelloTVTheme.text)
                        .lineLimit(1)
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
        .help("Mello TV overview")
    }

    private func sidebarSection<Content: View>(_ title: String,
                                               @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(MelloTVTheme.textSecondary)
                .padding(.leading, 12)
                .frame(height: 14)
                .opacity(expanded ? 1 : 0)
                .accessibilityHidden(!expanded)
                .accessibilityAddTraits(.isHeader)
            VStack(spacing: 3, content: content)
        }
    }

    private func destinationButton(_ destination: DesktopDestination) -> some View {
        SidebarRowButton(
            title: destination.rawValue,
            symbol: destination.symbol,
            expanded: expanded,
            selected: navigation.selection == destination
        ) {
            navigation.selection = destination
        }
        .keyboardShortcut(destination.shortcut, modifiers: .command)
    }
}

private struct SidebarRowButton: View {
    let title: String
    let symbol: String
    let expanded: Bool
    var selected = false
    var symbolColor: Color? = nil
    let action: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Image(systemName: symbol)
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(symbolColor ?? (selected ? MelloTVTheme.text : MelloTVTheme.textSecondary))
                    .frame(width: 20, height: 20)
                    .accessibilityHidden(true)
                if expanded {
                    Text(title)
                        .font(.system(size: 13, weight: selected ? .semibold : .regular))
                        .foregroundStyle(selected ? MelloTVTheme.text : MelloTVTheme.textSecondary)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                }
            }
            .frame(maxWidth: .infinity, alignment: expanded ? .leading : .center)
            .frame(height: 36)
            .padding(.horizontal, expanded ? 12 : 0)
            .contentShape(Rectangle())
        }
        .buttonStyle(SidebarRowStyle(selected: selected, hovered: hovered))
        .onHover { hovered = $0 }
        .help(title)
        .accessibilityLabel(title)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

private struct SidebarRowStyle: ButtonStyle {
    let selected: Bool
    let hovered: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(
                selected ? MelloTVTheme.selection : hovered || configuration.isPressed ? MelloTVTheme.quietFill : .clear,
                in: RoundedRectangle(cornerRadius: 9, style: .continuous)
            )
            .opacity(configuration.isPressed ? 0.65 : 1)
            .animation(reduceMotion ? nil : .easeOut(duration: 0.12), value: hovered)
            .animation(reduceMotion ? nil : .easeOut(duration: 0.12), value: configuration.isPressed)
    }
}
