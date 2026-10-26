import SwiftUI

struct DesktopShellView: View {
    @ObservedObject var navigation: DesktopNavigation
    @ObservedObject var model: ServerStatusStore
    @ObservedObject var process: ServerProcessState
    let port: Int
    let actions: AppActions
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(spacing: 0) {
            DesktopSidebarView(navigation: navigation, actions: actions)
                .frame(width: navigation.isSidebarExpanded ? 208 : 72)
                .padding(.top, 40)
            DesktopContentView(destination: $navigation.selection, model: model, process: process, port: port, actions: actions)
                .padding(.top, 40)
                .padding(.trailing, 10)
                .padding(.bottom, 10)
        }
        .background { SatinBackdrop().ignoresSafeArea() }
        .foregroundStyle(MelloTVTheme.text)
        .preferredColorScheme(actions.preferences.appearance.colorScheme)
        .animation(reduceMotion ? nil : .easeInOut(duration: 0.2), value: navigation.isSidebarExpanded)
    }
}
