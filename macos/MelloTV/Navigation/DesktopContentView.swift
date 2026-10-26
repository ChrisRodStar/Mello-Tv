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
            case .updates:
                UpdatesView(version: model.snapshot?.appVersion ?? Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "—", mode: model.snapshot?.providerUpdates.mode)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(MelloTVTheme.content)
        .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
        .overlay { RoundedRectangle(cornerRadius: 22, style: .continuous).strokeBorder(MelloTVTheme.hairline, lineWidth: 1) }
        .shadow(color: .black.opacity(0.06), radius: 16, y: 5)
    }
}
