import SwiftUI
import MelloKit

/// Main application root with a floating bottom navigation bar conforming to Mello TV iOS conventions.
/// Puts the tab bar at the bottom to give the 3D showroom stage full breathing room at the top.
public struct MainTabView: View {
    @State private var selectedTab: AppTab = .home
    @Namespace private var tabNamespace

    public init() {}

    public var body: some View {
        ZStack(alignment: .bottom) {
            // Active Tab Content Canvas
            Group {
                switch selectedTab {
                case .home:
                    CatalogView(category: .home)
                case .movies:
                    CatalogView(category: .movies)
                case .tv:
                    CatalogView(category: .tv)
                case .search:
                    SearchView()
                case .settings:
                    SettingsView()
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)

            // Bottom Dock Fade Scrim
            LinearGradient(
                stops: [
                    .init(color: .clear, location: 0.0),
                    .init(color: MelloTVTheme.background.opacity(0.85), location: 0.50),
                    .init(color: MelloTVTheme.background, location: 1.0)
                ],
                startPoint: .top,
                endPoint: .bottom
            )
            .frame(height: 140)
            .allowsHitTesting(false)

            // Floating Bottom Navigation Bar
            bottomTabBar
        }
        .edgesIgnoringSafeArea(.all)
        .background(MelloTVTheme.background.ignoresSafeArea())
    }

    // MARK: - Floating Bottom Navigation Bar

    @ViewBuilder
    private var bottomTabBar: some View {
        HStack(spacing: 12) {
            ForEach(AppTab.allCases) { tab in
                Button {
                    withAnimation(.spring(response: 0.32, dampingFraction: 0.78)) {
                        selectedTab = tab
                    }
                } label: {
                    HStack(spacing: 8) {
                        Image(systemName: tab.systemImage)
                            .font(.system(size: 17, weight: selectedTab == tab ? .bold : .medium))
                        Text(tab.title)
                            .font(.system(size: 16, weight: selectedTab == tab ? .bold : .semibold))
                    }
                    .padding(.horizontal, 22)
                    .padding(.vertical, 12)
                    .background(
                        selectedTab == tab
                            ? Color.white
                            : Color.white.opacity(0.08)
                    )
                    .foregroundStyle(
                        selectedTab == tab
                            ? Color.black
                            : Color.white.opacity(0.85)
                    )
                    .clipShape(Capsule())
                }
                .buttonStyle(TVBottomTabItemStyle(isSelected: selectedTab == tab))
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(.ultraThinMaterial)
        .environment(\.colorScheme, .dark)
        .background(Color.black.opacity(0.45))
        .clipShape(Capsule())
        .overlay(
            Capsule()
                .strokeBorder(Color.white.opacity(0.18), lineWidth: 0.75)
        )
        .shadow(color: Color.black.opacity(0.55), radius: 28, x: 0, y: 12)
        .padding(.bottom, 36)
        .zIndex(999)
    }
}

private struct TVBottomTabItemStyle: ButtonStyle {
    let isSelected: Bool

    func makeBody(configuration: Configuration) -> some View {
        TVBottomTabItemButton(configuration: configuration, isSelected: isSelected)
    }
}

private struct TVBottomTabItemButton: View {
    let configuration: ButtonStyle.Configuration
    let isSelected: Bool
    @Environment(\.isFocused) private var isFocused

    var body: some View {
        configuration.label
            .scaleEffect(isFocused ? 1.08 : (configuration.isPressed ? 0.96 : 1.0))
            .overlay(
                Capsule()
                    .strokeBorder(isFocused ? Color.white.opacity(0.8) : Color.clear, lineWidth: 2)
            )
            .shadow(color: isFocused ? Color.white.opacity(0.35) : Color.clear, radius: 10)
            .animation(.spring(response: 0.28, dampingFraction: 0.75), value: isFocused)
    }
}
