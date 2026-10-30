import Combine

final class ServerProcessState: ObservableObject {
    @Published var isRunning = false
    @Published var message: String?
    @Published var isRestarting = false
}
