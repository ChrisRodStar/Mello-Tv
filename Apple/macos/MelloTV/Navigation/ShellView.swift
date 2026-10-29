import SwiftUI
import MelloKit

struct ShellView: View {
    @ObservedObject var navigation: DesktopNavigation
    @ObservedObject var model: ServerStatusStore
    @ObservedObject var process: ServerProcessState
    let port: Int
    let actions: AppActions
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(spacing: 0) {
            SidebarView(navigation: navigation, actions: actions)
                .frame(width: navigation.isSidebarExpanded ? 208 : 72)
                .padding(.top, 6)
                .padding(.bottom, 8)
            ContentView(destination: $navigation.selection, model: model, process: process, port: port, actions: actions)
                .padding(.top, 6)
                .padding(.trailing, 8)
                .padding(.bottom, 8)
        }
        .background { SatinBackdrop().ignoresSafeArea() }
        .foregroundStyle(MelloTVTheme.text)
        .preferredColorScheme(actions.preferences.appearance.colorScheme)
        .animation(reduceMotion ? nil : .easeInOut(duration: 0.2), value: navigation.isSidebarExpanded)
    }
}

typealias DesktopShellView = ShellView
