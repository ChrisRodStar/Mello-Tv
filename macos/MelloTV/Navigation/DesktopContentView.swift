import SwiftUI

struct DesktopContentView: View {
    @Binding var destination: DesktopDestination
    @ObservedObject var model: ServerStatusStore
    @ObservedObject var process: ServerProcessState
    let port: Int
    let actions: AppActions

    var body: some View {
        Group {
            switch destination {
            case .overview:
                OverviewView(model: model, process: process, actions: actions, destination: $destination)
            case .providers:
                ProvidersView(providers: model.snapshot?.media.providers ?? [], online: process.isRunning && model.isReachable)
            case .network:
                NetworkView(port: port, endpoints: process.isRunning && model.isReachable ? model.snapshot?.endpoints ?? [] : [])
            case .settings:
                SettingsView(preferences: actions.preferences, process: process, snapshot: model.snapshot, actions: actions)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(MelloTVTheme.contentBackground)
        .clipShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .strokeBorder(MelloTVTheme.border, lineWidth: 1)
        }
        .shadow(color: Color.black.opacity(0.35), radius: 24, y: 8)
    }
}
