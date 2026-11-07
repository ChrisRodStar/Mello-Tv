import SwiftUI
import AVKit
import MelloKit

/// Native tvOS Video Player utilizing AVPlayerViewController for full Siri Remote integration.
public struct TVPlayerView: View {
    public let title: MediaTitle
    public var season: Int? = nil
    public var episode: Int? = nil
    public let onDismiss: () -> Void

    @State private var player: AVPlayer?
    @State private var isLoading = true
    @State private var errorMessage: String?
    @State private var sessionId = UUID().uuidString
    @State private var timeObserverToken: Any?

    public init(title: MediaTitle, season: Int? = nil, episode: Int? = nil, onDismiss: @escaping () -> Void) {
        self.title = title
        self.season = season
        self.episode = episode
        self.onDismiss = onDismiss
    }

    public var body: some View {
        ZStack {
            Color.black.edgesIgnoringSafeArea(.all)

            if let player = player {
                NativeTVPlayerRepresentable(player: player)
                    .edgesIgnoringSafeArea(.all)
            } else if isLoading {
                VStack(spacing: 20) {
                    ProgressView()
                    Text("Resolving stream on Mello TV server…")
                        .font(.headline)
                        .foregroundColor(.secondary)
                }
            } else if let errorMessage = errorMessage {
                VStack(spacing: 24) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .font(.system(size: 64))
                        .foregroundColor(.yellow)

                    Text("Unable to Play")
                        .font(.title2.weight(.bold))
                        .foregroundColor(.white)

                    Text(errorMessage)
                        .font(.body)
                        .foregroundColor(.secondary)
                        .multilineTextAlignment(.center)
                        .padding(.horizontal, 40)

                    Button("Dismiss", action: onDismiss)
                        .buttonStyle(.borderedProminent)
                }
            }
        }
        .task {
            await preparePlayback()
        }
        .onDisappear {
            teardownPlayback()
        }
    }

    private func preparePlayback() async {
        isLoading = true
        errorMessage = nil

        do {
            let streamURL = try await MelloTVAPI.shared.resolveStreamURL(
                type: title.type,
                id: title.tmdbID,
                season: season,
                episode: episode
            )

            let playerItem = AVPlayerItem(url: streamURL)
            let avPlayer = AVPlayer(playerItem: playerItem)
            self.player = avPlayer
            self.isLoading = false

            // Attach periodic progress observer
            let interval = CMTime(seconds: 5.0, preferredTimescale: CMTimeScale(NSEC_PER_SEC))
            timeObserverToken = avPlayer.addPeriodicTimeObserver(forInterval: interval, queue: .main) { time in
                let pos = CMTimeGetSeconds(time)
                let dur = avPlayer.currentItem?.duration.seconds ?? 0
                Task {
                    await MelloTVAPI.shared.updatePlaybackActivity(
                        sessionId: sessionId,
                        profileId: "default",
                        title: title.title,
                        season: season,
                        episode: episode,
                        state: "playing",
                        positionSeconds: pos,
                        durationSeconds: dur > 0 ? dur : nil
                    )
                }
            }

            // Auto-play
            avPlayer.play()
        } catch {
            self.isLoading = false
            self.errorMessage = error.localizedDescription
        }
    }

    private func teardownPlayback() {
        if let token = timeObserverToken {
            player?.removeTimeObserver(token)
            timeObserverToken = nil
        }
        player?.pause()
        player = nil

        let sid = sessionId
        Task {
            await MelloTVAPI.shared.stopPlaybackActivity(sessionId: sid)
        }
    }
}

/// SwiftUI wrapper for AVPlayerViewController to leverage native tvOS transport & Siri Remote.
struct NativeTVPlayerRepresentable: UIViewControllerRepresentable {
    let player: AVPlayer

    func makeUIViewController(context: Context) -> AVPlayerViewController {
        let controller = AVPlayerViewController()
        controller.player = player
        controller.showsPlaybackControls = true
        controller.requiresLinearPlayback = false
        controller.allowsPictureInPicturePlayback = false
        return controller
    }

    func updateUIViewController(_ uiViewController: AVPlayerViewController, context: Context) {
        uiViewController.player = player
    }
}
