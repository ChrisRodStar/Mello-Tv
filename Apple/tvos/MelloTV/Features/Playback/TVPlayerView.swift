import SwiftUI
import AVKit
import MelloKit

/// Native tvOS Video Player utilizing AVPlayerViewController with Siri Remote, Resume & Auto-Advance.
public struct TVPlayerView: View {
    public let title: MediaTitle
    @State public var season: Int?
    @State public var episode: Int?
    public var initialPosition: Double? = nil
    public let onDismiss: () -> Void

    @State private var player: AVPlayer?
    @State private var isLoading = true
    @State private var errorMessage: String?
    @State private var sessionId = UUID().uuidString
    @State private var timeObserverToken: Any?
    @State private var duration: Double = 0
    @State private var currentPosition: Double = 0

    // Auto-advance state for TV Series
    @State private var showNextEpisodePrompt = false
    @State private var countdownSeconds = 10
    @State private var countdownTimer: Timer?

    public init(
        title: MediaTitle,
        season: Int? = nil,
        episode: Int? = nil,
        initialPosition: Double? = nil,
        onDismiss: @escaping () -> Void
    ) {
        self.title = title
        self._season = State(initialValue: season)
        self._episode = State(initialValue: episode)
        self.initialPosition = initialPosition
        self.onDismiss = onDismiss
    }

    public var body: some View {
        ZStack {
            Color.black.edgesIgnoringSafeArea(.all)

            if let player = player {
                NativeTVPlayerRepresentable(player: player)
                    .edgesIgnoringSafeArea(.all)

                // Next Episode Binge-Watching Overlay
                if showNextEpisodePrompt, let nextEp = nextEpisodeNumber {
                    VStack {
                        Spacer()
                        HStack {
                            Spacer()
                            VStack(alignment: .leading, spacing: 12) {
                                Text("Up Next in \(countdownSeconds)s")
                                    .font(.caption.weight(.bold))
                                    .foregroundColor(.secondary)
                                    .textCase(.uppercase)

                                Text("Season \(season ?? 1) Episode \(nextEp)")
                                    .font(.headline.weight(.heavy))
                                    .foregroundColor(.white)

                                HStack(spacing: 16) {
                                    Button {
                                        playNextEpisodeNow()
                                    } label: {
                                        HStack(spacing: 8) {
                                            Image(systemName: "play.fill")
                                            Text("Play Now")
                                                .fontWeight(.bold)
                                        }
                                    }
                                    .buttonStyle(.borderedProminent)

                                    Button {
                                        cancelAutoAdvance()
                                    } label: {
                                        Text("Cancel")
                                    }
                                    .buttonStyle(.bordered)
                                }
                            }
                            .padding(24)
                            .background(.ultraThinMaterial)
                            .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
                            .overlay(
                                RoundedRectangle(cornerRadius: 16, style: .continuous)
                                    .stroke(Color.white.opacity(0.3), lineWidth: 1)
                            )
                            .shadow(color: .black.opacity(0.7), radius: 24, y: 12)
                            .padding(48)
                        }
                    }
                    .transition(.move(edge: .trailing).combined(with: .opacity))
                }
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
        .task(id: "\(title.id)-\(season ?? 0)-\(episode ?? 0)") {
            await preparePlayback()
        }
        .onDisappear {
            teardownPlayback()
        }
    }

    private var nextEpisodeNumber: Int? {
        guard title.type == .tv, let current = episode else { return nil }
        return current + 1
    }

    private func preparePlayback() async {
        isLoading = true
        errorMessage = nil
        showNextEpisodePrompt = false
        countdownTimer?.invalidate()
        countdownTimer = nil

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

            // Seek to resume position if provided
            if let pos = initialPosition, pos > 10.0 {
                let targetTime = CMTime(seconds: pos, preferredTimescale: 1)
                await avPlayer.seek(to: targetTime, toleranceBefore: .zero, toleranceAfter: .zero)
            }

            // Periodic observer for progress reporting & end detection
            let interval = CMTime(seconds: 2.0, preferredTimescale: CMTimeScale(NSEC_PER_SEC))
            timeObserverToken = avPlayer.addPeriodicTimeObserver(forInterval: interval, queue: .main) { [self] time in
                let pos = CMTimeGetSeconds(time)
                let dur = avPlayer.currentItem?.duration.seconds ?? 0
                self.currentPosition = pos
                self.duration = dur

                // Trigger next episode prompt near end
                if title.type == .tv && dur > 60 && pos >= (dur - 25) && !showNextEpisodePrompt {
                    startNextEpisodeCountdown()
                }

                // Heartbeat to Mello TV server
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

            // End-of-stream notification
            NotificationCenter.default.addObserver(
                forName: .AVPlayerItemDidPlayToEndTime,
                object: playerItem,
                queue: .main
            ) { _ in
                if title.type == .tv && nextEpisodeNumber != nil {
                    playNextEpisodeNow()
                } else {
                    onDismiss()
                }
            }

            avPlayer.play()
        } catch {
            self.isLoading = false
            self.errorMessage = error.localizedDescription
        }
    }

    private func startNextEpisodeCountdown() {
        withAnimation {
            showNextEpisodePrompt = true
            countdownSeconds = 10
        }
        countdownTimer?.invalidate()
        countdownTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { _ in
            if countdownSeconds > 1 {
                countdownSeconds -= 1
            } else {
                playNextEpisodeNow()
            }
        }
    }

    private func playNextEpisodeNow() {
        countdownTimer?.invalidate()
        countdownTimer = nil
        showNextEpisodePrompt = false

        if let next = nextEpisodeNumber {
            teardownPlayback()
            self.episode = next
            self.sessionId = UUID().uuidString
            // Re-triggers task
        }
    }

    private func cancelAutoAdvance() {
        countdownTimer?.invalidate()
        countdownTimer = nil
        withAnimation {
            showNextEpisodePrompt = false
        }
    }

    private func teardownPlayback() {
        countdownTimer?.invalidate()
        countdownTimer = nil

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

/// Native AVPlayerViewController integration for tvOS with Siri Remote controls.
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
