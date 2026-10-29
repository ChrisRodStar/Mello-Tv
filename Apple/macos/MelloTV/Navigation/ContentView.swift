import SwiftUI
import MelloKit

struct ContentView: View {
    @Binding var destination: DesktopDestination
    @ObservedObject var model: ServerStatusStore
    @ObservedObject var process: ServerProcessState
    let port: Int
    let actions: AppActions
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        Group {
            switch destination {
            case .overview:
                OverviewView(model: model, process: process, actions: actions, destination: $destination)
            case .providers:
                ProvidersView(providers: model.snapshot?.media.providers ?? [], online: process.isRunning && model.isReachable, starting: model.isStarting, port: port)
            case .network:
                NetworkView(port: port, endpoints: model.snapshot?.endpoints ?? [],
                            online: process.isRunning && model.isReachable, starting: model.isStarting)
            case .users:
                UsersView(online: process.isRunning && model.isReachable, starting: model.isStarting, port: port)
            case .settings:
                SettingsView(preferences: actions.preferences, updater: actions.updater, process: process, snapshot: model.snapshot, actions: actions)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(MelloTVTheme.contentBackground)
        .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 22, style: .continuous)
                .strokeBorder(MelloTVTheme.border, lineWidth: 1)
        }
        .shadow(color: Color.black.opacity(colorScheme == .dark ? 0.35 : 0.05), radius: 16, y: 4)
    }
}

typealias DesktopContentView = ContentView
