import SwiftUI

@main
struct MelloTVApp: App {
    @StateObject private var serverAddress = ServerAddress.shared

    var body: some Scene {
        WindowGroup {
            Group {
                if serverAddress.activeBaseURL != nil {
                    MainTabView()
                } else {
                    ServerDiscoveryView()
                }
            }
            .preferredColorScheme(.dark)
        }
    }
}
