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
            DesktopSidebarView(navigation: navigation, model: model, process: process, actions: actions)
                .frame(width: navigation.isSidebarExpanded ? 208 : 72)
            DesktopContentView(destination: $navigation.selection, model: model, process: process, port: port, actions: actions)
                .padding(.top, 8)
                .padding(.trailing, 8)
                .padding(.bottom, 8)
        }
        .frame(minWidth: 1150, minHeight: 750)
        .background(MelloTVTheme.workspace.ignoresSafeArea())
        .foregroundStyle(.primary)
        .preferredColorScheme(.dark)
        .animation(reduceMotion ? nil : .easeInOut(duration: 0.2), value: navigation.isSidebarExpanded)
    }
}
