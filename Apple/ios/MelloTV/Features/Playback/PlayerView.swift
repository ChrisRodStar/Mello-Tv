import SwiftUI
import AVKit
import Combine
import MediaPlayer
import Observation
import OSLog
import MelloKit

private let playbackLog = Logger(subsystem: Bundle.main.bundleIdentifier ?? "com.tinymellow.mellotv", category: "Playback")

@MainActor
struct PlayerView: View {
    let request: PlaybackRequest
    var onDismiss: () -> Void
    // The presentation owns episode changes. A new episode gets a fresh player
    // lifetime while the chosen speed remains owned by this presentation.
    @State private var activeRequest: PlaybackRequest
    @State private var playbackSpeed: Float = 1
    @State private var playbackQuality: PlaybackQuality = .automatic
    @State private var nextPreparation = NextEpisodePreparation()
    @State private var activePreparation: PreparedNextEpisode?
    @State private var playbackSettings = ProfilePlaybackSettings()
    @State private var settingsLoaded = false

    init(request: PlaybackRequest, onDismiss: @escaping () -> Void) {
        let timedRequest = request.timing == nil ? request.traced(.presentation) : request
        self.request = timedRequest
        self.onDismiss = onDismiss
        _activeRequest = State(initialValue: timedRequest)
    }

    var body: some View {
        ZStack {
            if settingsLoaded { PlaybackSessionView(request: activeRequest, settings: playbackSettings, playbackSpeed: $playbackSpeed, playbackQuality: $playbackQuality,
                            nextPreparation: nextPreparation, preparedEpisode: activePreparation,
                            onDismiss: onDismiss, onPlayEpisode: { next in
                                activePreparation = nextPreparation.claim(for: next)
                                activeRequest = next
                            })
                .id(activeRequest.id) } else { ProgressView().tint(.white) }
        }
        .background { PlaybackLandscapeLock().allowsHitTesting(false) }
        .preferredColorScheme(.dark)
        .task {
            do {
                playbackSettings = try await MelloTVAPI(baseURL: ServerAddress.baseURL).playbackSettings(profileID: request.profileID)
            } catch {
                var fallback = ProfilePlaybackSettings()
                // Do not autoplay when the saved preference could not be read.
                fallback.autoplayNext = false
                playbackSettings = fallback
            }
            guard !Task.isCancelled else { return }
            playbackQuality = PlaybackQuality(rawValue: playbackSettings.maxHeight) ?? .automatic
            settingsLoaded = true
        }
        .onDisappear { nextPreparation.cancel() }
    }
}

@MainActor
private struct PlaybackSessionView: View {
    let request: PlaybackRequest
    let settings: ProfilePlaybackSettings
    @Binding var playbackSpeed: Float
    @Binding var playbackQuality: PlaybackQuality
    let nextPreparation: NextEpisodePreparation
    let preparedEpisode: PreparedNextEpisode?
    let onDismiss: () -> Void
    let onPlayEpisode: (PlaybackRequest) -> Void

    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.accessibilityVoiceOverEnabled) private var voiceOverEnabled
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    @State private var gestureSettings = PlayerGestureSettings()
    @State private var adjustmentState = PlayerAdjustmentState()
    @State private var isAdjusting = false
    @State private var adjustmentFeedbackTask: Task<Void, Never>?
    @State private var player: AVPlayer?
    @State private var isPlaybackSceneActive = false
    @State private var progressTracker: PlaybackProgressTracker?
    @State private var isPlaying = false
    @State private var intendsToPlay = true
    @State private var isLoading = true
    @State private var isBuffering = false
    @State private var hasStartedPlayback = false
    @State private var errorMessage: String?
    @State private var showControls = true
    @State private var currentTime: Double = 0
    @State private var duration: Double = 0
    @State private var isSeeking = false
    @State private var sliderValue: Double = 0
    @State private var seekFeedback: (direction: Int, seconds: Int, id: UUID)?
    @State private var seekQueue = PlaybackSeekQueue()
    @State private var seekTimeoutState = PlaybackSeekTimeoutState()
    @State private var seekTimeoutTask: Task<Void, Never>?
    @State private var playbackClockStall = PlaybackClockStallDetector()
    @State private var hideControlsTask: Task<Void, Never>?
    @State private var feedbackTask: Task<Void, Never>?
    @State private var itemStatusCancellable: AnyCancellable?
    @State private var playerStatusCancellable: AnyCancellable?
    @State private var preparationTask: Task<Void, Never>?
    @State private var renewalTask: Task<Void, Never>?
    @State private var automaticUpgradeTask: Task<Void, Never>?
    @State private var automaticUpgradeAttempted = false
    @State private var sessionFinalizationTask: Task<Void, Never>?
    @State private var recoveryTask: Task<Void, Never>?
    @State private var stallTask: Task<Void, Never>?
    @State private var playbackClockWatchdogTask: Task<Void, Never>?
    @State private var operations = PlaybackSessionOperations()
    @State private var recoveryAttempts = 0
    @State private var lastAutomaticRecreation: TimeInterval = -.infinity
    @State private var creationKey = UUID().uuidString.lowercased()
    @State private var preparationConsumed = false
    @State private var startupTime = Date()
    @State private var timing: PlaybackTiming?
    @State private var lastObservedPlaybackTime: Double?
    @State private var recordedFirstFrame = false
    @State private var trackTask: Task<Void, Never>?
    @State private var timeObserver: Any?
    @State private var endObserver: NSObjectProtocol?
    @State private var accessLogObserver: NSObjectProtocol?
    @State private var authorizedSource: AuthorizedPlayback?
    @State private var acceptedPreparation = false
    @State private var activePanel: PlaybackPanel?
    @State private var pendingEpisode: PlaybackRequest?
    @State private var isLocked = false
    @State private var showUnlockControl = true
    @State private var unlockHideTask: Task<Void, Never>?
    @State private var nextEpisodeTask: Task<Void, Never>?
    @State private var isLoadingNextEpisode = false
    @State private var nextEpisodeCandidate: PlaybackRequest?
    @State private var nextCandidateTask: Task<Void, Never>?
    @State private var nextEpisodeLookup = NextEpisodeLookupState()
    @State private var nextCandidateToken: UUID?
    @State private var episodePrompt = EpisodePromptState()
    @State private var episodePromptTask: Task<Void, Never>?
    @State private var episodePromptToken: UUID?
    @State private var isAtNaturalEnd = false
    @State private var playbackSegments = PlaybackSegments.empty(duration: 0)
    @State private var segmentTask: Task<Void, Never>?
    @State private var loadedSegmentItem: ObjectIdentifier?
    @State private var episodeMessage: String?
    @State private var audioGroup: AVMediaSelectionGroup?
    @State private var subtitleGroup: AVMediaSelectionGroup?
    @State private var selectedAudio: AVMediaSelectionOption?
    @State private var selectedSubtitle: AVMediaSelectionOption?
    @State private var audioTrackPreference: String?
    @State private var subtitleTrackPreference: String?
    @State private var subtitleOffSelected = false
    @State private var loadingTracks = false

    private let preparedPlayback = PreparedPlaybackService.shared
    private var isPreparing: Bool { errorMessage == nil && !hasStartedPlayback && (isLoading || intendsToPlay) }
    private var controlsVisible: Bool { isPreparing || errorMessage != nil || (!isLocked && showControls) }
    private var canSeek: Bool { duration.isFinite && duration > 0 && hasStartedPlayback && !isLocked }

    var body: some View {
        GeometryReader { geometry in
            ZStack {
                Color.black.ignoresSafeArea()

                if let player {
                    VideoSurface(player: player, gestureSettings: gestureSettings, timing: timing) {
                        if !recordedFirstFrame {
                            recordedFirstFrame = true
                            playbackLog.info("First frame after \(Date().timeIntervalSince(startupTime), privacy: .public) seconds")
                        }
                        if !hasStartedPlayback {
                            hasStartedPlayback = true
                            isBuffering = false
                            scheduleAutoHide()
                            considerAutomaticQualityUpgrade()
                        }
                    }
                        .overlay {
                            PlayerTouchSurface(
                                adjustmentsEnabled: hasStartedPlayback && errorMessage == nil && !voiceOverEnabled && !isLocked,
                                onTap: toggleControls,
                                onDoubleTap: { location, width in
                                    handleDoubleTap(at: location, width: width)
                                },
                                onAdjustmentBegan: beginAdjustment,
                                onAdjustmentChanged: updateAdjustment,
                                onAdjustmentEnded: endAdjustment
                            )
                            .accessibilityHidden(true)
                        }
                        .ignoresSafeArea()
                        .accessibilityLabel("Video")
                        .accessibilityAction(named: "Show playback controls") { toggleControls() }
                        .accessibilityAction(named: "Back 10 seconds") { seek(by: -10) }
                        .accessibilityAction(named: "Forward 10 seconds") { seek(by: 10) }
                }

                if controlsVisible {
                    LinearGradient(stops: [
                        .init(color: .black.opacity(0.46), location: 0),
                        .init(color: .black.opacity(0.08), location: 0.34),
                        .init(color: .black.opacity(0.08), location: 0.58),
                        .init(color: .black.opacity(0.62), location: 1)
                    ], startPoint: .top, endPoint: .bottom)
                        .ignoresSafeArea()
                        .allowsHitTesting(false)
                }

                if isPreparing {
                    preparationOverlay
                } else if let errorMessage {
                    errorOverlay(errorMessage)
                } else if controlsVisible {
                    transportControls
                }

                if controlsVisible {
                    VStack(spacing: 0) {
                        header
                        Spacer(minLength: 12)
                        if hasStartedPlayback && errorMessage == nil {
                            timeline
                        }
                    }
                    .padding(.horizontal, geometry.size.width > 600 ? 32 : 20)
                    .padding(.top, 12)
                    .padding(.bottom, 8)
                    .transition(.opacity)
                }

                if isLocked && showUnlockControl && errorMessage == nil {
                    VStack {
                        Spacer()
                        Button {
                            isLocked = false
                            unlockHideTask?.cancel()
                            showControls = true
                            scheduleAutoHide()
                        } label: {
                            Label {
                                Text("Unlock controls")
                            } icon: {
                                PhosphorIcon("lock", size: 16, weight: .fill)
                            }
                            .font(.subheadline.weight(.semibold))
                            .padding(.horizontal, 20)
                            .frame(minHeight: 48)
                            .playerControlSurface(in: Capsule())
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                    .padding(.bottom, 24)
                    .transition(.opacity)
                }

                PlayerAdjustmentIndicator(state: adjustmentState)
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)

                if let feedback = seekFeedback {
                    HStack {
                        if feedback.direction > 0 { Spacer() }
                        Label {
                            Text("\(feedback.direction > 0 ? "+" : "−")\(feedback.seconds)s")
                        } icon: {
                            PhosphorIcon(feedback.direction < 0 ? "skip-back" : "skip-forward", size: 18, weight: .fill)
                        }
                        .font(.subheadline.monospacedDigit().weight(.semibold))
                        .padding(12)
                        .shadow(color: .black.opacity(0.8), radius: 3, y: 1)
                        .transition(.opacity)
                        if feedback.direction < 0 { Spacer() }
                    }
                    .padding(.horizontal, 24)
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
                }
                if let panel = activePanel, panel != .episodes {
                    compactPanel(panel, in: geometry.size)
                        .transition(.opacity)
                        .zIndex(10)
                }

                if hasStartedPlayback, errorMessage == nil, isPlaybackSceneActive,
                   !isLocked, activePanel == nil, !isSeeking,
                   seekQueue.pendingTarget == nil, !isAdjusting {
                    episodeContextPrompts(in: geometry.size)
                        .transition(.opacity)
                        .zIndex(8)
                }
            }
            .foregroundStyle(.white)
        }
        .statusBarHidden(!controlsVisible)
        .animation(reduceMotion ? nil : .easeInOut(duration: 0.2), value: controlsVisible)
        .animation(reduceMotion ? nil : .easeOut(duration: 0.15), value: activePanel)
        .onChange(of: playbackQuality) { _, quality in
            if let item = player?.currentItem { applyQuality(quality, to: item) }
            timing?.record(.qualityLimitChanged, value: Double(quality.height))
            if quality == .automatic { considerAutomaticQualityUpgrade() }
        }
        .task { startPlayback() }
        .onDisappear { cleanup() }
        .onChange(of: scenePhase, initial: true) { _, phase in
            isPlaybackSceneActive = phase == .active
            if phase == .background {
                playbackClockStall.reset()
                if isSeeking, seekQueue.activeRequest == nil {
                    isSeeking = false
                    sliderValue = currentTime
                    progressTracker?.setSeeking(false)
                }
                episodePrompt.backgrounded(at: currentTime)
                episodePromptTask?.cancel()
                episodePromptTask = nil
                episodePromptToken = nil
                nextCandidateTask?.cancel()
                nextCandidateTask = nil
                nextCandidateToken = nil
                nextEpisodeLookup.fail(now: ProcessInfo.processInfo.systemUptime)
                segmentTask?.cancel()
                segmentTask = nil
                loadedSegmentItem = nil
                playbackSegments = .empty(duration: 0)
                nextPreparation.cancel()
                finishAdjustment(immediately: true)
                gestureSettings.restoreBrightness()
                intendsToPlay = false
                player?.pause()
                progressTracker?.flush()
            } else if phase == .active {
                if let seek = seekQueue.activeRequest, let player {
                    startSeekTimeout(for: seek, on: player)
                }
                playbackClockStall.reset()
                if let source = authorizedSource {
                    if source.expiresAt <= Date() { recoverPlayback() }
                    else if source.expiresAt.timeIntervalSinceNow < 90 { startRenewal(immediate: true) }
                }
            }
        }
        .onChange(of: voiceOverEnabled) { _, enabled in
            if enabled {
                showControls = true
                if isLocked { revealUnlockControl() }
            }
            scheduleAutoHide()
        }
        .onChange(of: playbackSpeed) { _, speed in
            player?.defaultRate = speed
            if intendsToPlay { player?.play() }
        }
        .onChange(of: activePanel) { previous, panel in
            if panel == nil && previous != .episodes { scheduleAutoHide() }
        }
        .sheet(isPresented: Binding(
            get: { activePanel == .episodes },
            set: { if !$0 && activePanel == .episodes { activePanel = nil } }
        ), onDismiss: panelDidDismiss) {
            PlaybackEpisodesSheet(request: request,
                playingProgress: duration > 0 ? min(1, max(0, currentTime / duration)) : 0) { season, episode in
                playEpisode(episode, season: season)
            }
        }
        .alert("Episodes", isPresented: Binding(
            get: { episodeMessage != nil },
            set: { if !$0 { episodeMessage = nil } }
        )) {
            Button("OK", role: .cancel) {
                episodeMessage = nil
                scheduleAutoHide()
            }
        } message: {
            Text(episodeMessage ?? "")
        }
    }

    private var header: some View {
        HStack(spacing: 12) {
            Button(action: dismiss) {
                PhosphorIcon("x", size: 17, weight: .bold)
                    .frame(width: 48, height: 48)
                    .playerControlSurface(in: Circle())
                    .contentShape(Circle())
            }
            .accessibilityLabel("Close player")

            VStack(alignment: .leading, spacing: 3) {
                Text(request.title)
                    .font(.headline.weight(.semibold))
                    .lineLimit(1)
                if request.type == .tv, let season = request.season, let episode = request.episode {
                    Text("S\(season) · E\(episode)\(request.episodeTitle.map { " · \($0)" } ?? "")")
                        .font(.caption)
                        .foregroundStyle(.white.opacity(0.70))
                        .lineLimit(1)
                }
            }
            .accessibilityElement(children: .combine)
            Spacer(minLength: 0)

            if hasStartedPlayback && errorMessage == nil {
                Button {
                    isLocked = true
                    showControls = false
                    nextEpisodeTask?.cancel()
                    finishAdjustment(immediately: true)
                    hideControlsTask?.cancel()
                    revealUnlockControl()
                } label: {
                    PhosphorIcon("lock-open", size: 20)
                        .frame(width: 48, height: 48)
                        .playerControlSurface(in: Circle())
                        .contentShape(Circle())
                }
                .accessibilityLabel("Lock playback controls")
            }
        }
        .buttonStyle(.plain)
    }

    private var preparationOverlay: some View {
        VStack(spacing: 14) {
            ProgressView().tint(.white)
            Text(isLoading ? "Finding a playable source" : "Preparing video")
                .font(.subheadline.weight(.medium))
            Text("\(request.title)")
                .font(.caption)
                .foregroundStyle(MelloTVTheme.muted)
                .lineLimit(2)
        }
        .multilineTextAlignment(.center)
        .padding(.horizontal, 32)
        .accessibilityElement(children: .combine)
    }

    private func errorOverlay(_ message: String) -> some View {
        VStack(spacing: 16) {
            PhosphorIcon("prohibit", size: 32)
                .foregroundStyle(MelloTVTheme.muted)
            Text("Couldn’t play this title")
                .font(.headline)
            Text(message)
                .font(.subheadline)
                .foregroundStyle(MelloTVTheme.textSecondary)
                .multilineTextAlignment(.center)
            Button("Try again") {
                if authorizedSource != nil {
                    recoveryAttempts = 0
                    lastAutomaticRecreation = -.infinity
                    recoverPlayback()
                } else {
                    startPlayback(retry: true)
                }
            }
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(.black)
                .padding(.horizontal, 24)
                .frame(minHeight: 44)
                .background(.white, in: Capsule())
        }
        .frame(maxWidth: 360)
        .padding(24)
    }

    private var transportControls: some View {
        Button(action: togglePlayPause) {
            ZStack {
                if isBuffering && intendsToPlay {
                    ProgressView().tint(.white)
                } else {
                    PhosphorIcon(intendsToPlay ? "pause" : "play", size: 34, weight: .fill)
                }
            }
            .frame(width: 76, height: 76)
            .shadow(color: .black.opacity(0.65), radius: 4, y: 1)
            .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(intendsToPlay ? "Pause" : "Play")
        .accessibilityValue(isBuffering ? "Buffering" : "")
    }

    private func beginAdjustment(_ kind: PlayerAdjustment.Kind) {
        guard !isLocked else { return }
        guard let initial = gestureSettings.initialValue(for: kind) else { return }
        adjustmentFeedbackTask?.cancel()
        hideControlsTask?.cancel()
        isAdjusting = true
        adjustmentState.adjustment = PlayerAdjustment(kind: kind, value: initial)
        adjustmentState.isVisible = true
    }

    private func updateAdjustment(_ delta: CGFloat, _ travel: CGFloat) {
        guard isAdjusting, !isLocked, var current = adjustmentState.adjustment else { return }
        // Sensitivity belongs to the gesture area, not the small HUD. A short
        // movement now makes a small change in either orientation.
        current.value = min(1, max(0, current.value - Float(delta / travel)))
        adjustmentState.adjustment = current
        // Publish visual feedback first. Coalesce device writes at display
        // cadence instead of running synchronous system setters per touch event.
        gestureSettings.schedule(current.value, for: current.kind)
        if current.kind == .volume, current.value > 0, let player, player.isMuted {
            player.isMuted = false
        }
    }

    private func endAdjustment() {
        guard isAdjusting else { return }
        isAdjusting = false
        finishAdjustment()
    }

    private func finishAdjustment(immediately: Bool = false) {
        adjustmentFeedbackTask?.cancel()
        if immediately {
            gestureSettings.cancelPendingUpdates()
            adjustmentState.isVisible = false
            adjustmentState.adjustment = nil
            isAdjusting = false
            return
        }
        gestureSettings.finishUpdates()
        adjustmentFeedbackTask = Task {
            do { try await Task.sleep(for: .milliseconds(700)) } catch { return }
            adjustmentState.isVisible = false
            scheduleAutoHide()
        }
    }

    private var timeline: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                PlayerSeekSlider(value: Binding(
                    get: { isSeeking ? sliderValue : currentTime },
                    set: { value in
                        if !isSeeking {
                            isSeeking = true
                            progressTracker?.setSeeking(true)
                        }
                        sliderValue = value
                    }
                ), upperBound: max(duration, 1)) { editing in
                    hideControlsTask?.cancel()
                    if editing {
                        episodePrompt.seekOrScrub(to: currentTime)
                        episodePromptTask?.cancel()
                        episodePromptTask = nil
                        episodePromptToken = nil
                        sliderValue = currentTime
                        isSeeking = true
                        progressTracker?.setSeeking(true)
                    } else {
                        seekTo(sliderValue)
                        isSeeking = false
                    }
                }
                .tint(MelloTVTheme.accent)
                .frame(minHeight: 44)
                .disabled(!canSeek)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("Playback position")
                .accessibilityValue("\(formatSeconds(isSeeking ? sliderValue : currentTime)) of \(formatSeconds(duration))")
                .accessibilityAdjustableAction { direction in
                    switch direction {
                    case .increment: seek(by: 10)
                    case .decrement: seek(by: -10)
                    @unknown default: break
                    }
                }

                let position = isSeeking ? sliderValue : currentTime
                Text("−" + formatSeconds(max(0, duration - position)))
                    .font(.caption.monospacedDigit())
                    .frame(minWidth: 44, alignment: .trailing)
                    .accessibilityLabel("\(formatSeconds(max(0, duration - position))) remaining")
            }
            // The seek target extends into the empty video above this group,
            // leaving the action targets below it separate and unobstructed.
            .frame(height: 44)
            .offset(y: -6)
            .frame(height: 32)
            ViewThatFits(in: .horizontal) {
                playbackActions(compact: false)
                playbackActions(compact: true)
                ScrollView(.horizontal) { playbackActions(compact: false) }
                    .scrollIndicators(.hidden)
            }
        }
    }

    private func playbackActions(compact: Bool) -> some View {
        HStack(spacing: compact ? 8 : 16) {
            actionButton("\(speedLabel)×", icon: "speedometer", compact: compact) { openPanel(.speed) }
                .accessibilityLabel("Playback speed, \(speedLabel) times")
            if authorizedSource?.format == "hls", !(authorizedSource?.availableHeights.isEmpty ?? true) {
                actionButton(playbackQuality.label, icon: "stack", compact: compact) { openPanel(.quality) }
                    .accessibilityLabel("Video quality, \(playbackQuality.label)")
            }
            if request.type == .tv {
                actionButton("Episodes", icon: "stack", compact: compact) { openPanel(.episodes) }
            }
            actionButton(compact ? "Audio" : "Audio & subtitles", icon: "subtitles", compact: compact) {
                openPanel(.tracks)
            }
            .accessibilityLabel("Audio and subtitles")
            if request.type == .tv {
                Button(action: nextEpisode) {
                    actionLabel(compact ? "Next" : "Next episode", icon: "skip-forward", compact: compact)
                        .opacity(isLoadingNextEpisode ? 0 : 1)
                        .overlay {
                            if isLoadingNextEpisode { ProgressView().tint(.white) }
                        }
                }
                .disabled(isLoadingNextEpisode)
                .accessibilityLabel("Next episode")
            }
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 12)
        .padding(.vertical, 2)
        .frame(maxWidth: .infinity)
        .playerControlSurface(in: Capsule())
    }

    private func actionButton(_ title: String, icon: String, compact: Bool,
                              action: @escaping () -> Void) -> some View {
        Button(action: action) { actionLabel(title, icon: icon, compact: compact) }
    }

    private func actionLabel(_ title: String, icon: String, compact: Bool) -> some View {
        HStack(spacing: compact ? 5 : 7) {
            PhosphorIcon(icon, size: compact ? 14 : 16, weight: .bold)
            Text(title)
                .font(compact ? .caption2.weight(.medium) : .caption.weight(.medium))
        }
        .fixedSize()
        .frame(maxWidth: .infinity, minHeight: 44)
        .contentShape(Rectangle())
    }

    private var speedLabel: String { String(format: "%g", Double(playbackSpeed)) }

    private func compactPanel(_ panel: PlaybackPanel, in size: CGSize) -> some View {
        ZStack(alignment: .bottom) {
            Button { activePanel = nil } label: {
                Color.black.opacity(0.16).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Dismiss playback menu")
            .ignoresSafeArea()

            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 12) {
                    Text(panel == .speed ? "Playback speed" : (panel == .quality ? "Video quality" : "Audio & subtitles"))
                        .font(.headline)
                    Spacer(minLength: 8)
                    Button { activePanel = nil } label: {
                        PhosphorIcon("x", size: 16, weight: .bold)
                            .touchTarget()
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Close playback menu")
                }
                if panel == .speed { speedPicker }
                else if panel == .quality { qualityPicker }
                else { trackPicker }
            }
            .padding(.horizontal, 18)
            .padding(.bottom, 18)
            .padding(.top, 16)
            .frame(width: min(panel == .speed ? 420 : 520, max(0, size.width - 32)))
            .playerControlSurface(in: RoundedRectangle(cornerRadius: 24, style: .continuous))
            .shadow(color: .black.opacity(0.3), radius: 16, y: 8)
            .padding(.bottom, 12)
            .accessibilityElement(children: .contain)
            .accessibilityAddTraits(.isModal)
        }
    }

    private var speedPicker: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 4) {
                ForEach([Float(0.5), 0.75, 1, 1.25, 1.5, 2], id: \.self) { speed in
                    Button {
                        playbackSpeed = speed
                        activePanel = nil
                    } label: {
                        Text("\(String(format: "%g", Double(speed)))×")
                            .font(.subheadline.weight(playbackSpeed == speed ? .bold : .medium))
                            .foregroundStyle(playbackSpeed == speed ? MelloTVTheme.accent : .white)
                            .frame(minWidth: 54, minHeight: 48)
                            .background(playbackSpeed == speed ? Color.white.opacity(0.08) : .clear,
                                        in: RoundedRectangle(cornerRadius: 8))
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(speed == 1 ? "Normal speed" : "\(String(format: "%g", Double(speed))) times speed")
                    .accessibilityAddTraits(playbackSpeed == speed ? [.isSelected] : [])
                }
            }
        }
        .scrollIndicators(.hidden)
        .frame(height: 48)
    }

    private var qualityPicker: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                ForEach(PlaybackQuality.allCases.filter { $0 == .automatic || authorizedSource?.availableHeights.contains($0.height) == true }) { quality in
                    Button {
                        playbackQuality = quality
                        activePanel = nil
                    } label: {
                        Text(quality.label)
                            .font(.subheadline.weight(playbackQuality == quality ? .bold : .medium))
                            .foregroundStyle(playbackQuality == quality ? MelloTVTheme.accent : .white)
                            .frame(maxWidth: .infinity, minHeight: 48)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityAddTraits(playbackQuality == quality ? [.isSelected] : [])
                }
            }
            Text("Quality adapts to your connection and the available stream.")
                .font(.caption)
                .foregroundStyle(MelloTVTheme.muted)
        }
    }

    private func applyQuality(_ quality: PlaybackQuality, to item: AVPlayerItem) {
        quality.apply(to: item)
    }

    private func considerAutomaticQualityUpgrade() {
        guard playbackQuality == .automatic, hasStartedPlayback, isPlaybackSceneActive,
              !automaticUpgradeAttempted, automaticUpgradeTask == nil,
              let source = authorizedSource,
              !source.availableHeights.contains(1080), source.expiresAt.timeIntervalSinceNow > 90,
              let activePlayer = player else { return }
        automaticUpgradeAttempted = true
        automaticUpgradeTask = Task {
            defer { automaticUpgradeTask = nil }
            do {
                let upgraded = try await operations.upgrade(source, request: request)
                try Task.checkCancellation()
                guard self.player === activePlayer, authorizedSource?.revision == source.revision,
                      upgraded.id == source.id, upgraded.reloadRequired,
                      upgraded.revision > source.revision else { return }
                authorizedSource = upgraded

                // The server changes the relay source only after discovery
                // finds a strictly higher measured rendition. Adopt its signed
                // URL even when the new source is MP4 and has no HLS height list.
                captureTrackPreferences(from: activePlayer.currentItem)
                let position = activePlayer.currentTime().seconds
                let resumePosition = position.isFinite ? max(0, position) : currentTime
                activePlayer.pause()
                let replacement = AVPlayerItem(url: upgraded.url)
                applyQuality(playbackQuality, to: replacement)
                activePlayer.replaceCurrentItem(with: replacement)
                observeItem(replacement)
                await seekForPlayback(activePlayer, seconds: resumePosition)
                try Task.checkCancellation()
                if intendsToPlay { activePlayer.play() }
            } catch {
                // A missing higher rendition or a transient probe failure does
                // not interrupt the already-playing source.
            }
        }
    }

    private func openPanel(_ panel: PlaybackPanel) {
        hideControlsTask?.cancel()
        nextEpisodeTask?.cancel()
        activePanel = panel
    }

    private func playEpisode(_ episode: Episode, season: Int) {
        let next = PlaybackRequest(titleID: request.titleID, type: .tv,
            title: request.title, profileID: request.profileID, season: season,
            episode: episode.number, episodeTitle: episode.name, catalogTitle: request.catalogTitle).traced(.nextEpisode)
        if activePanel != nil {
            // Let the episode sheet finish dismissing before replacing its
            // presenting player, rather than interrupting that transition.
            pendingEpisode = next
            activePanel = nil
        } else {
            switchToEpisode(next)
        }
    }

    private func panelDidDismiss() {
        if let next = pendingEpisode {
            pendingEpisode = nil
            switchToEpisode(next)
        } else {
            scheduleAutoHide()
        }
    }

    private func switchToEpisode(_ next: PlaybackRequest) {
        episodePrompt.playNextNow()
        episodePromptTask?.cancel()
        episodePromptTask = nil
        episodePromptToken = nil
        nextCandidateTask?.cancel()
        nextCandidateTask = nil
        nextCandidateToken = nil
        player?.pause()
        progressTracker?.flush()
        onPlayEpisode(next)
    }

    private func nextEpisode() {
        guard request.type == .tv, !isLoadingNextEpisode,
              request.season != nil, request.episode != nil else { return }
        hideControlsTask?.cancel()
        isLoadingNextEpisode = true
        let nextTiming = PlaybackTiming(action: .nextEpisode, titleID: request.titleID, type: request.type)
        nextTiming.record(.resumeMetadataStarted)
        nextEpisodeTask = Task {
            defer {
                isLoadingNextEpisode = false
                if !Task.isCancelled { scheduleAutoHide() }
            }
            do {
                guard let next = try await nextPreparation.nextRequest(after: request) else {
                    episodeMessage = "You’ve reached the last available episode."
                    return
                }
                try Task.checkCancellation()
                nextTiming.record(.resumeMetadataFinished, flag: true)
                switchToEpisode(next.traced(.nextEpisode, using: nextTiming))
            } catch {
                guard !Task.isCancelled else { return }
                episodeMessage = error.localizedDescription
            }
        }
    }

    private func revealUnlockControl() {
        showUnlockControl = true
        unlockHideTask?.cancel()
        guard !voiceOverEnabled else { return }
        unlockHideTask = Task {
            do { try await Task.sleep(for: .seconds(3)) } catch { return }
            showUnlockControl = false
        }
    }

    private var trackPicker: some View {
        Group {
            if loadingTracks {
                ProgressView("Loading available tracks")
                    .tint(.white)
                    .frame(maxWidth: .infinity, minHeight: 92)
            } else {
                HStack(alignment: .top, spacing: 20) {
                    trackColumn("Audio", group: audioGroup, isSubtitle: false)
                    trackColumn("Subtitles", group: subtitleGroup, isSubtitle: true)
                }
            }
        }
    }

    private func episodeContextPrompts(in size: CGSize) -> some View {
        VStack(spacing: 10) {
            Spacer(minLength: 0)
            HStack(alignment: .bottom) {
                if let intro = activeIntroSegment {
                    Button {
                        seekTo(intro.end, tolerance: 0)
                    } label: {
                        Label(intro.kind == .recap ? "Skip Recap" : "Skip Intro", systemImage: "forward.end.fill")
                            .font(.subheadline.weight(.semibold))
                            .padding(.horizontal, 16)
                            .frame(minHeight: 46)
                            .background(Color.black.opacity(0.78), in: Capsule())
                    }
                    .buttonStyle(.plain)
                    .accessibilityHint(intro.kind == .recap ? "Skip to the end of the recap" : "Skip to the end of the intro")
                }

                Spacer(minLength: 12)

                if episodePrompt.isVisible, let nextEpisodeCandidate {
                    Button { playPreparedNext(nextEpisodeCandidate) } label: {
                        Label("Play next", systemImage: "play.fill")
                            .font(.subheadline.weight(.semibold))
                            .frame(width: 148, height: 46)
                            .background {
                                GeometryReader { geometry in
                                    ZStack(alignment: .leading) {
                                        Color.black.opacity(0.82)
                                        Color.white.opacity(0.24)
                                            .frame(width: geometry.size.width * nextEpisodeCountdownProgress)
                                    }
                                }
                                .clipShape(RoundedRectangle(cornerRadius: 12))
                            }
                            .overlay {
                                RoundedRectangle(cornerRadius: 12)
                                    .strokeBorder(Color.white.opacity(0.22), lineWidth: 1)
                            }
                    }
                    .buttonStyle(.plain)
                    .animation(reduceMotion ? nil : .linear(duration: 1), value: nextEpisodeCountdownProgress)
                    .accessibilityLabel("Play next episode")
                    .accessibilityValue(settings.autoplayNext && episodePrompt.hasNaturallyEarnedCountdown
                        ? "Automatically playing in \(episodePrompt.secondsRemaining) seconds" : "")
                }
            }
        }
        .frame(maxWidth: min(size.width - 32, 720))
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
        .padding(.horizontal, 16)
        .padding(.bottom, controlsVisible ? 112 : 22)
    }

    private var activeIntroSegment: PlaybackSegment? {
        guard duration.isFinite, duration > 0, playbackSegments.duration > 0,
              abs(playbackSegments.duration - duration) <= max(2, duration * 0.005),
              !isSeeking, seekQueue.pendingTarget == nil, !isAdjusting,
              let intro = playbackSegments.segments.first(where: {
                  ($0.kind == .intro || $0.kind == .recap) && currentTime >= $0.start && currentTime < $0.end
              }) else { return nil }
        return intro
    }

    private var nextEpisodeCountdownProgress: CGFloat {
        guard settings.autoplayNext, episodePrompt.hasNaturallyEarnedCountdown else { return 0 }
        return CGFloat(EpisodePromptState.countdownLength - episodePrompt.secondsRemaining)
            / CGFloat(EpisodePromptState.countdownLength)
    }

    private func loadPlaybackSegments(for item: AVPlayerItem, duration itemDuration: Double) {
        guard request.type == .tv, itemDuration.isFinite, itemDuration > 0 else { return }
        let itemID = ObjectIdentifier(item)
        guard loadedSegmentItem != itemID else { return }
        loadedSegmentItem = itemID
        let ownerKey = request.cacheKey
        let segmentsRequest = request
        segmentTask?.cancel()
        segmentTask = Task {
            let result = await PlaybackSegmentService.shared.segments(for: segmentsRequest, duration: itemDuration)
            guard !Task.isCancelled, request.cacheKey == ownerKey,
                  player?.currentItem === item else { return }
            playbackSegments = result
            playbackLog.info("Episode timing lookup finished for S\(segmentsRequest.season ?? 0) E\(segmentsRequest.episode ?? 0): \(result.segments.count, privacy: .public) accepted markers")
            updateEpisodePrompt(position: currentTime, duration: duration, naturallyAdvanced: false)
        }
    }

    private func updateEpisodePrompt(position: Double, duration itemDuration: Double, naturallyAdvanced: Bool) {
        guard request.type == .tv, isPlaybackSceneActive, hasStartedPlayback,
              errorMessage == nil,
              nextEpisodeCandidate != nil,
              itemDuration.isFinite, itemDuration > 0,
              position.isFinite, player?.timeControlStatus == .playing,
              !isLocked, activePanel == nil, !isSeeking, seekQueue.pendingTarget == nil,
              !isAdjusting else { return }
        let creditsStart = playbackSegments.segments.first(where: { $0.kind == .credits })?.start
        let speed = Double(max(0.5, playbackSpeed))
        let lastMinute = itemDuration - 60 * speed
        let cardTrigger = max(0, min(lastMinute, creditsStart ?? lastMinute))
        let countdownTrigger = max(0, itemDuration - 10 * speed)
        episodePrompt.consider(position: position, cardTrigger: cardTrigger,
                               countdownTrigger: countdownTrigger, naturallyAdvanced: naturallyAdvanced)
        if episodePrompt.isVisible { ensureEpisodePromptTimer() }
    }

    private func ensureEpisodePromptTimer() {
        guard episodePromptTask == nil else { return }
        let ownerKey = request.cacheKey
        let activePlayer = player
        let activeItem = player?.currentItem
        let token = UUID()
        episodePromptToken = token
        episodePromptTask = Task {
            defer {
                if episodePromptToken == token {
                    episodePromptTask = nil
                    episodePromptToken = nil
                }
            }
            while !Task.isCancelled, episodePrompt.isVisible {
                do { try await Task.sleep(for: .seconds(1)) } catch { return }
                guard !Task.isCancelled, request.cacheKey == ownerKey,
                      self.player === activePlayer, self.player?.currentItem === activeItem,
                      isPlaybackSceneActive, hasStartedPlayback, errorMessage == nil else { return }
                guard episodePrompt.isVisible, (isPlaying || isAtNaturalEnd), !isLocked, activePanel == nil,
                      !isSeeking, seekQueue.pendingTarget == nil, !isAdjusting else { continue }
                if settings.autoplayNext, isAtNaturalEnd, episodePrompt.hasNaturallyEarnedCountdown,
                   episodePrompt.secondsRemaining == 0 {
                    guard let nextEpisodeCandidate else { return }
                    playPreparedNext(nextEpisodeCandidate)
                    return
                }
                if settings.autoplayNext, episodePrompt.tick(), isAtNaturalEnd {
                    continue
                }
            }
        }
    }

    private func playPreparedNext(_ next: PlaybackRequest) {
        episodePrompt.playNextNow()
        episodePromptTask?.cancel()
        episodePromptTask = nil
        episodePromptToken = nil
        switchToEpisode(next.traced(.nextEpisode))
    }

    private func trackColumn(_ title: String, group: AVMediaSelectionGroup?, isSubtitle: Bool) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .font(.caption.weight(.semibold))
                .foregroundStyle(MelloTVTheme.muted)
            ScrollView {
                VStack(alignment: .leading, spacing: 2) {
                    if let group, !group.options.isEmpty {
                        if isSubtitle && group.allowsEmptySelection {
                            trackButton("Off", option: nil, group: group, isSubtitle: true)
                        }
                        trackOptions(group, isSubtitle: isSubtitle)
                    } else {
                        Text(isSubtitle ? "No subtitles for this source" : "Default audio")
                            .font(.subheadline)
                            .foregroundStyle(MelloTVTheme.muted)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.vertical, 12)
                    }
                }
            }
            .frame(height: 132)
            .scrollIndicators(.hidden)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func trackOptions(_ group: AVMediaSelectionGroup, isSubtitle: Bool) -> some View {
        ForEach(group.options, id: \.self) { option in
            trackButton(option.displayName, option: option, group: group, isSubtitle: isSubtitle)
        }
    }

    private func trackButton(_ title: String, option: AVMediaSelectionOption?, group: AVMediaSelectionGroup,
                             isSubtitle: Bool) -> some View {
        let selected = (isSubtitle ? selectedSubtitle : selectedAudio) == option
        return Button {
            guard let item = player?.currentItem else { return }
            item.select(option, in: group)
            if isSubtitle {
                selectedSubtitle = option
                subtitleOffSelected = option == nil
                subtitleTrackPreference = option.map(trackPreferenceKey)
            } else {
                selectedAudio = option
                audioTrackPreference = option.map(trackPreferenceKey)
            }
        } label: {
            HStack {
                Text(title)
                Spacer()
                if selected { PhosphorIcon("check", size: 14, weight: .bold) }
            }
            .font(.subheadline)
            .foregroundStyle(selected ? MelloTVTheme.accent : .white)
            .frame(maxWidth: .infinity)
            .touchTarget()
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }

    // MARK: - Playback Logic

    private func startPlayback(retry: Bool = false, resumeAt: Double? = nil, playWhenReady: Bool = true) {
        if hasStartedPlayback { episodePrompt.backgrounded(at: currentTime) }
        cleanup(invalidateRequest: false, abandonPreparation: false)
        let trace = (retry ? nil : request.timing) ?? PlaybackTiming(
            action: retry ? .retry : .presentation, titleID: request.titleID, type: request.type,
            season: request.season, episode: request.episode)
        timing = trace
        trace.record(.presentationStarted)
        lastObservedPlaybackTime = nil
        let tracedRequest = request.traced(.presentation, using: trace)
        startupTime = Date()
        recordedFirstFrame = false
        isLoading = true
        isBuffering = false
        hasStartedPlayback = false
        intendsToPlay = playWhenReady
        isPlaying = false
        currentTime = 0
        sliderValue = 0
        duration = 0
        isSeeking = false
        showControls = true
        errorMessage = nil

        preparationTask = Task {
            do {
                let source: AuthorizedPlayback
                var prewarmedPlayer: AVPlayer?
                var preparedPosition: Double?
                if let preparedEpisode, !preparationConsumed {
                    preparationConsumed = true
                    let ready: AuthorizedPlayback?
                    do {
                        trace.record(.preparationClaimStarted, related: preparedEpisode.diagnosticTiming)
                        ready = try await preparedEpisode.resolve(for: request)
                    } catch {
                        try Task.checkCancellation()
                        ready = nil
                    }
                    if let ready {
                        trace.record(.preparationClaimed, related: preparedEpisode.diagnosticTiming, flag: false)
                        source = ready
                    } else {
                        // Reconcile an ambiguous speculative reply with its same
                        // key. Expired known sessions instead get a fresh key.
                        let prepared = try await preparedPlayback.resolve(request: tracedRequest,
                            idempotencyKey: preparedEpisode.fallbackKey ?? creationKey)
                        source = prepared.source
                        prewarmedPlayer = prepared.player
                        preparedPosition = prepared.preparedPosition
                    }
                } else {
                    let prepared = try await preparedPlayback.resolve(request: tracedRequest, idempotencyKey: creationKey)
                    source = prepared.source
                    prewarmedPlayer = prepared.player
                    preparedPosition = prepared.preparedPosition
                }
                try Task.checkCancellation()
                authorizedSource = source
                trace.record(.preparationReady, flag: prewarmedPlayer != nil)
                trace.record(.renditionsOffered, flag: source.availableHeights.count > 1, value: Double(source.availableHeights.count))
                playbackLog.info("Authorization ready after \(Date().timeIntervalSince(startupTime), privacy: .public) seconds; prepared player: \(prewarmedPlayer != nil, privacy: .public)")
                acceptedPreparation = preparedEpisode != nil

                let resumePosition = resumeAt ?? (request.startFromBeginning ? 0 : source.resumeSeconds)
                let newPlayer: AVPlayer
                if let prewarmedPlayer {
                    newPlayer = prewarmedPlayer
                } else {
                    let playerItem = AVPlayerItem(url: source.url)
                    playerItem.canUseNetworkResourcesForLiveStreamingWhilePaused = true
                    newPlayer = AVPlayer(playerItem: playerItem)
                }
                if let item = newPlayer.currentItem { applyQuality(playbackQuality, to: item) }
                trace.record(.playerCreated, flag: prewarmedPlayer != nil)
                let currentPosition = newPlayer.currentTime().seconds
                let positioned = preparedPosition.map { abs($0 - resumePosition) <= 1 } == true
                    && currentPosition.isFinite && abs(currentPosition - resumePosition) <= 1
                if !positioned && (resumePosition > 0 || (currentPosition.isFinite && currentPosition > 1)) {
                    trace.record(.resumeSeekStarted)
                    await seekForPlayback(newPlayer, seconds: resumePosition)
                    trace.record(.resumeSeekFinished)
                }
                guard !Task.isCancelled else {
                    newPlayer.pause()
                    newPlayer.replaceCurrentItem(with: nil)
                    return
                }
                newPlayer.defaultRate = playbackSpeed
                self.player = newPlayer
                startPlaybackClockWatchdog(for: newPlayer)
                trace.record(.playerAttached)
                isLoading = false
                isBuffering = true
                playerStatusCancellable = newPlayer.publisher(for: \.timeControlStatus)
                    .receive(on: DispatchQueue.main)
                    .sink { status in
                        guard self.player === newPlayer, errorMessage == nil else { return }
                        isPlaying = status == .playing
                        isBuffering = status == .waitingToPlayAtSpecifiedRate
                        if isPlaying {
                            isAtNaturalEnd = false
                            trace.record(.playing, width: Double(newPlayer.currentItem?.presentationSize.width ?? 0),
                                         height: Double(newPlayer.currentItem?.presentationSize.height ?? 0))
                            hasStartedPlayback = true
                        } else if isBuffering { trace.record(.waiting) }
                        monitorStall(newPlayer)
                        if !intendsToPlay { showControls = true }
                        scheduleAutoHide()
                    }

                if let currentItem = newPlayer.currentItem {
                    observeItem(currentItem)
                }

                let tracker = PlaybackProgressTracker(player: newPlayer, request: request, source: source)
                self.progressTracker = tracker
                tracker.start()

                // Set up periodic time observer
                let interval = CMTime(seconds: 0.5, preferredTimescale: 600)
                timeObserver = newPlayer.addPeriodicTimeObserver(forInterval: interval, queue: .main) { time in
                    Task { @MainActor in
                        guard self.player === newPlayer else { return }
                        let size = newPlayer.currentItem?.presentationSize ?? .zero
                        trace.record(.qualityChanged, width: Double(size.width), height: Double(size.height))
                        if !isSeeking, seekQueue.pendingTarget == nil, time.seconds.isFinite {
                            let previousTime = lastObservedPlaybackTime
                            let playbackDelta = previousTime.map { time.seconds - $0 }
                            let naturallyAdvanced = playbackDelta.map {
                                $0 > 0.01 && $0 <= max(2, Double(max(0.5, playbackSpeed)) * 2)
                            } ?? false
                            if naturallyAdvanced,
                               newPlayer.timeControlStatus == .playing {
                                trace.record(.playbackAdvancing)
                            }
                            lastObservedPlaybackTime = time.seconds
                            self.currentTime = max(0, time.seconds)
                            if let dur = newPlayer.currentItem?.duration.seconds, dur > 0, dur.isFinite {
                                self.duration = dur
                                self.progressTracker?.handlePlaybackItemReady()
                                if let currentItem = newPlayer.currentItem {
                                    self.loadPlaybackSegments(for: currentItem, duration: dur)
                                }
                                considerNextEpisodePreparation()
                                updateEpisodePrompt(position: time.seconds, duration: dur,
                                    naturallyAdvanced: naturallyAdvanced)
                            }
                        }
                    }
                }

                if intendsToPlay {
                    trace.record(.playRequested)
                    newPlayer.play()
                }
                startRenewal()
            } catch {
                trace.record(Task.isCancelled ? .cancelled : .failed)
                guard !Task.isCancelled else { return }
                self.isLoading = false
                self.errorMessage = error.localizedDescription
                self.preparedPlayback.invalidate(request: request)
            }
        }
    }

    private func considerNextEpisodePreparation() {
        guard request.type == .tv, isPlaybackSceneActive,
              player?.timeControlStatus == .playing, !isSeeking,
              duration.isFinite, duration > 0, currentTime.isFinite,
              authorizedSource != nil else { return }
        let remaining = duration - currentTime
        guard remaining > 0, remaining / Double(max(0.5, playbackSpeed)) <= 120 else { return }
        nextPreparation.prepare(after: request)
        guard nextCandidateTask == nil,
              nextEpisodeLookup.beginAttempt(now: ProcessInfo.processInfo.systemUptime) else { return }
        playbackLog.info("Next episode metadata lookup started for S\(request.season ?? 0) E\(request.episode ?? 0), attempt \(self.nextEpisodeLookup.attempts, privacy: .public)")
        let owner = request.cacheKey
        let activePlayer = player
        let activeItem = player?.currentItem
        let token = UUID()
        nextCandidateToken = token
        nextCandidateTask = Task {
            defer {
                if nextCandidateToken == token {
                    nextCandidateTask = nil
                    nextCandidateToken = nil
                }
            }
            do {
                let candidate = try await nextPreparation.nextRequest(after: request)
                try Task.checkCancellation()
                guard nextCandidateToken == token, request.cacheKey == owner,
                      self.player === activePlayer,
                      self.player?.currentItem === activeItem else { return }
                nextEpisodeLookup.complete(foundSuccessor: candidate != nil)
                nextEpisodeCandidate = candidate
                if let candidate {
                    playbackLog.info("Next episode metadata ready for S\(candidate.season ?? 0) E\(candidate.episode ?? 0), attempt \(self.nextEpisodeLookup.attempts, privacy: .public)")
                } else {
                    playbackLog.info("No next episode was found; caching the no-successor result")
                }
            } catch {
                guard nextCandidateToken == token, request.cacheKey == owner,
                      self.player === activePlayer,
                      self.player?.currentItem === activeItem else { return }
                nextEpisodeLookup.fail(now: ProcessInfo.processInfo.systemUptime)
                let retryDelay = nextEpisodeLookup.retryAt.map {
                    max(0, $0 - ProcessInfo.processInfo.systemUptime)
                } ?? 0
                playbackLog.error("Next episode metadata lookup failed on attempt \(self.nextEpisodeLookup.attempts, privacy: .public); retry in \(retryDelay, format: .fixed(precision: 1)) seconds")
            }
        }
    }

    private func observeItem(_ item: AVPlayerItem) {
        nextCandidateTask?.cancel()
        nextCandidateTask = nil
        nextCandidateToken = nil
        nextEpisodeCandidate = nil
        nextEpisodeLookup = NextEpisodeLookupState()
        segmentTask?.cancel()
        segmentTask = nil
        loadedSegmentItem = nil
        playbackSegments = .empty(duration: 0)
        episodePromptTask?.cancel()
        episodePromptTask = nil
        episodePromptToken = nil
        episodePrompt.bind(to: request.cacheKey)
        episodePrompt.seekOrScrub(to: currentTime)
        isAtNaturalEnd = false
        seekQueue.cancel()
        seekTimeoutTask?.cancel()
        seekTimeoutTask = nil
        seekTimeoutState.reset()
        isSeeking = false
        sliderValue = currentTime
        progressTracker?.setSeeking(false)
        playbackClockStall.reset()
        let trace = timing
        applyQuality(playbackQuality, to: item)
        if let accessLogObserver { NotificationCenter.default.removeObserver(accessLogObserver) }
        accessLogObserver = NotificationCenter.default.addObserver(forName: .AVPlayerItemNewAccessLogEntry, object: item, queue: .main) { _ in
            Task { @MainActor in
                guard player?.currentItem === item, let event = item.accessLog()?.events.last else { return }
                trace?.record(.deliverySample, flag: event.numberOfStalls > 0, value: event.observedBitrate,
                              width: Double(item.presentationSize.width), height: Double(item.presentationSize.height))
            }
        }
        itemStatusCancellable?.cancel()
        if let endObserver { NotificationCenter.default.removeObserver(endObserver) }
        itemStatusCancellable = item.publisher(for: \.status)
            .receive(on: DispatchQueue.main)
            .sink { status in
                guard player?.currentItem === item else { return }
                if status == .readyToPlay {
                    trace?.record(.itemReady, width: item.presentationSize.width, height: item.presentationSize.height)
                    loadTracks(for: item)
                    let itemDuration = item.duration.seconds
                    if itemDuration.isFinite, itemDuration > 0 {
                        loadPlaybackSegments(for: item, duration: itemDuration)
                    }
                } else if status == .failed {
                    isPlaying = false
                    isLoading = false
                    recoverPlayback()
                }
            }
        endObserver = NotificationCenter.default.addObserver(
            forName: .AVPlayerItemDidPlayToEndTime, object: item, queue: .main
        ) { _ in
            Task { @MainActor in
                guard player?.currentItem === item else { return }
                let hadVisiblePrompt = episodePrompt.isVisible
                isAtNaturalEnd = true
                intendsToPlay = false
                isPlaying = false
                showControls = true
                progressTracker?.flush()
                if !hadVisiblePrompt {
                    episodePrompt.seekOrScrub(to: currentTime)
                } else {
                    episodePrompt.naturalEnd()
                    ensureEpisodePromptTimer()
                }
            }
        }
    }

    private func startRenewal(immediate: Bool = false) {
        let previous = renewalTask
        previous?.cancel()
        renewalTask = Task {
            await previous?.value
            var shouldWait = !immediate
            while !Task.isCancelled {
                if shouldWait {
                    do { try await Task.sleep(for: .seconds(30)) } catch { return }
                }
                shouldWait = true
                guard let source = authorizedSource else { return }
                guard source.expiresAt.timeIntervalSinceNow < 90 else { continue }
                do {
                    let renewed = try await operations.renew(source, request: request)
                    try Task.checkCancellation()
                    authorizedSource = renewed
                    // Reload from the same authorized relay session with its renewed
                    // grant; preserve the tracker/cumulative progress identity.
                    if renewed.url != source.url, let player {
                        let position = player.currentTime()
                        let shouldResume = intendsToPlay
                        let item = AVPlayerItem(url: renewed.url)
                        player.replaceCurrentItem(with: item)
                        observeItem(item)
                        await seekForPlayback(player, seconds: position.seconds)
                        guard !Task.isCancelled else { return }
                        if shouldResume && intendsToPlay { player.play() }
                    }
                } catch {
                    if Task.isCancelled { return }
                    if source.expiresAt <= Date() {
                        player?.pause()
                        progressTracker?.flush()
                        recoverPlayback()
                        return
                    }
                }
            }
        }
    }

    private func monitorStall(_ observedPlayer: AVPlayer) {
        stallTask?.cancel()
        stallTask = nil
        guard intendsToPlay, observedPlayer.timeControlStatus == .waitingToPlayAtSpecifiedRate else { return }
        stallTask = Task {
            do { try await Task.sleep(for: .seconds(15)) } catch { return }
            guard self.player === observedPlayer, intendsToPlay,
                  observedPlayer.timeControlStatus == .waitingToPlayAtSpecifiedRate else { return }
            recoverPlayback()
        }
    }

    /// Runs independently of AVPlayer's periodic media-time callbacks, which
    /// may stop arriving when the playback clock itself is stalled.
    private func startPlaybackClockWatchdog(for observedPlayer: AVPlayer) {
        playbackClockWatchdogTask?.cancel()
        playbackClockStall.reset()
        playbackClockWatchdogTask = Task { @MainActor in
            while !Task.isCancelled, self.player === observedPlayer {
                do { try await Task.sleep(for: .seconds(1)) } catch { return }
                guard !Task.isCancelled, self.player === observedPlayer else { return }
                let position = observedPlayer.currentTime().seconds
                let didStall = playbackClockStall.observe(
                    position: position,
                    now: ProcessInfo.processInfo.systemUptime,
                    expectsPlayback: intendsToPlay && hasStartedPlayback
                        && observedPlayer.timeControlStatus == .playing,
                    isForeground: isPlaybackSceneActive,
                    isSeeking: isSeeking || seekQueue.pendingTarget != nil
                )
                if didStall {
                    playbackClockStall.reset()
                    playbackLog.error("Playback clock stopped while AVPlayer reported active playback; starting bounded recovery")
                    recoverPlayback()
                }
            }
        }
    }

    private func recoverPlayback() {
        guard recoveryTask == nil, let source = authorizedSource, let player else { return }
        if source.expiresAt <= Date() { recreateExpiredPlayback(); return }
        guard recoveryAttempts < 2 else {
            errorMessage = "Playback was interrupted. Try again."
            return
        }
        recoveryAttempts += 1
        timing?.record(.recoveryStarted)
        playbackLog.info("Recovering playback; attempt \(recoveryAttempts, privacy: .public)")
        let position = player.currentTime().seconds
        let savedPosition = hasStartedPlayback ? currentTime : (request.startFromBeginning ? 0 : source.resumeSeconds)
        let resumePosition = position.isFinite ? max(0, position) : savedPosition
        progressTracker?.flush()
        player.pause()
        errorMessage = nil
        isLoading = false
        isBuffering = true
        showControls = true
        let previousRenewal = renewalTask
        previousRenewal?.cancel()
        renewalTask = nil
        let previousUpgrade = automaticUpgradeTask
        previousUpgrade?.cancel()
        recoveryTask = Task {
            defer { recoveryTask = nil }
            // Settle the old owner before reconciling its pending mutation.
            await previousRenewal?.value
            await previousUpgrade?.value
            do {
                try Task.checkCancellation()
                let recovered = try await operations.recover(source, request: request)
                try Task.checkCancellation()
                guard self.player === player else { return }
                authorizedSource = recovered
                let item = AVPlayerItem(url: recovered.url)
                player.replaceCurrentItem(with: item)
                observeItem(item)
                await seekForPlayback(player, seconds: resumePosition)
                try Task.checkCancellation()
                if intendsToPlay { player.play() }
                startRenewal()
            } catch {
                guard !Task.isCancelled else { return }
                if (error as? APIError)?.isExpiredPlayback == true {
                    recreateExpiredPlayback()
                    return
                }
                errorMessage = error.localizedDescription
                isBuffering = false
            }
        }
    }

    private func recreateExpiredPlayback() {
        let now = ProcessInfo.processInfo.systemUptime
        guard now - lastAutomaticRecreation > 30 else {
            errorMessage = "Playback couldn’t be restored. Try again."
            return
        }
        lastAutomaticRecreation = now
        let position = player?.currentTime().seconds
        let resume = position.flatMap { $0.isFinite ? max(0, $0) : nil } ?? currentTime
        let shouldPlay = intendsToPlay
        creationKey = UUID().uuidString.lowercased()
        operations = PlaybackSessionOperations()
        recoveryAttempts = 0
        startPlayback(retry: true, resumeAt: resume, playWhenReady: shouldPlay)
    }

    // Both cancellation and a bounded deadline end a seek before an obsolete
    // player can resume. A seek failure remains observable through item status.
    private func seekForPlayback(_ player: AVPlayer, seconds: Double) async {
        guard seconds.isFinite, seconds >= 0, let item = player.currentItem else { return }
        let timeout = Task { @MainActor in
            do { try await Task.sleep(for: .seconds(8)) } catch { return }
            item.cancelPendingSeeks()
        }
        defer { timeout.cancel() }
        await withTaskCancellationHandler {
            _ = await player.seek(to: CMTime(seconds: seconds, preferredTimescale: 600),
                                  toleranceBefore: .positiveInfinity, toleranceAfter: .positiveInfinity)
        } onCancel: {
            Task { @MainActor in item.cancelPendingSeeks() }
        }
    }

    private func togglePlayPause() {
        guard let player else { return }
        intendsToPlay.toggle()
        if intendsToPlay {
            isAtNaturalEnd = false
            player.play()
        } else {
            player.pause()
            progressTracker?.flush()
        }
        showControls = true
        scheduleAutoHide()
    }

    private func seek(by deltaSeconds: Double) {
        guard let player, canSeek,
              let target = seekQueue.target(skipping: deltaSeconds,
                  from: player.currentTime().seconds, duration: duration) else { return }
        seekTo(target, tolerance: 0.5)
    }

    private func seekTo(_ targetSeconds: Double, tolerance: Double = 0) {
        guard let player, canSeek, targetSeconds.isFinite else { return }
        isAtNaturalEnd = false
        episodePrompt.seekOrScrub(to: targetSeconds)
        episodePromptTask?.cancel()
        episodePromptTask = nil
        episodePromptToken = nil
        hideControlsTask?.cancel()
        progressTracker?.setSeeking(true)
        let target = max(0, min(duration, targetSeconds))
        currentTime = target
        if let request = seekQueue.enqueue(target: target, tolerance: tolerance) {
            performSeek(request, on: player)
        }
    }

    private func performSeek(_ request: PlaybackSeekQueue.Request, on player: AVPlayer) {
        guard let item = player.currentItem else {
            seekQueue.cancel()
            seekTimeoutTask?.cancel()
            seekTimeoutTask = nil
            seekTimeoutState.reset()
            isSeeking = false
            sliderValue = currentTime
            progressTracker?.setSeeking(false)
            return
        }
        startSeekTimeout(for: request, on: player)
        let tolerance = CMTime(seconds: request.tolerance, preferredTimescale: 600)
        player.seek(to: CMTime(seconds: request.target, preferredTimescale: 600),
                    toleranceBefore: tolerance, toleranceAfter: tolerance) { _ in
            Task { @MainActor in
                guard self.player === player, player.currentItem === item else { return }
                let completion = seekQueue.complete(request.id)
                guard completion.accepted else { return }
                seekTimeoutTask?.cancel()
                seekTimeoutTask = nil
                _ = seekTimeoutState.complete(request.id)
                if let next = completion.next {
                    performSeek(next, on: player)
                } else {
                    let actualPosition = player.currentTime().seconds
                    if !isSeeking, actualPosition.isFinite {
                        currentTime = max(0, actualPosition)
                        sliderValue = currentTime
                        lastObservedPlaybackTime = actualPosition
                    }
                    progressTracker?.setSeeking(isSeeking)
                    scheduleAutoHide()
                }
            }
        }
    }

    private func startSeekTimeout(for request: PlaybackSeekQueue.Request, on player: AVPlayer) {
        guard let item = player.currentItem else { return }
        seekTimeoutState.begin(request.id, now: ProcessInfo.processInfo.systemUptime)
        seekTimeoutTask?.cancel()
        seekTimeoutTask = Task {
            do { try await Task.sleep(for: .seconds(PlaybackSeekTimeoutState.timeout)) } catch { return }
            guard !Task.isCancelled, self.player === player, player.currentItem === item,
                  seekQueue.activeRequest?.id == request.id,
                  seekTimeoutState.expire(request.id, now: ProcessInfo.processInfo.systemUptime,
                                          isForeground: isPlaybackSceneActive) else { return }

            item.cancelPendingSeeks()
            seekQueue.cancel()
            seekTimeoutTask = nil
            let actualPosition = player.currentTime().seconds
            if actualPosition.isFinite {
                currentTime = max(0, actualPosition)
                sliderValue = currentTime
                lastObservedPlaybackTime = actualPosition
            }
            isSeeking = false
            progressTracker?.setSeeking(false)
            playbackClockStall.reset()
            playbackLog.error("Player seek completed without its callback; reconciled timeline to AVPlayer clock")
            scheduleAutoHide()
        }
    }

    private func handleDoubleTap(at location: CGPoint, width: CGFloat) {
        guard canSeek else { return }
        if location.x < width / 2 {
            seek(by: -10)
            triggerSeekFeedback(direction: -1)
        } else {
            seek(by: 10)
            triggerSeekFeedback(direction: 1)
        }
    }

    private func triggerSeekFeedback(direction: Int) {
        let id = UUID()
        let seconds = seekFeedback.flatMap { $0.direction == direction ? $0.seconds + 10 : nil } ?? 10
        seekFeedback = (direction: direction, seconds: seconds, id: id)
        feedbackTask?.cancel()
        feedbackTask = Task {
            do { try await Task.sleep(for: .milliseconds(750)) } catch { return }
            if seekFeedback?.id == id {
                seekFeedback = nil
            }
        }
    }

    private func toggleControls() {
        if isLocked {
            revealUnlockControl()
            return
        }
        guard hasStartedPlayback, errorMessage == nil, isPlaying, !voiceOverEnabled else {
            showControls = true
            return
        }
        showControls.toggle()
        if showControls {
            scheduleAutoHide()
        } else {
            hideControlsTask?.cancel()
        }
    }

    private func scheduleAutoHide() {
        hideControlsTask?.cancel()
        guard isPlaying, !isSeeking, !isAdjusting, !isLoadingNextEpisode, !isLocked, activePanel == nil, !voiceOverEnabled, errorMessage == nil, episodeMessage == nil else { return }
        hideControlsTask = Task {
            do { try await Task.sleep(for: .seconds(4)) } catch { return }
            guard isPlaying, !isSeeking, !isAdjusting, !isLoadingNextEpisode, !isLocked, activePanel == nil, !voiceOverEnabled, errorMessage == nil, episodeMessage == nil else { return }
            showControls = false
        }
    }

    private func loadTracks(for item: AVPlayerItem) {
        trackTask?.cancel()
        audioGroup = nil
        subtitleGroup = nil
        selectedAudio = nil
        selectedSubtitle = nil
        loadingTracks = true
        trackTask = Task {
            let audio = try? await item.asset.loadMediaSelectionGroup(for: .audible)
            guard !Task.isCancelled, player?.currentItem === item else { return }
            let subtitles = try? await item.asset.loadMediaSelectionGroup(for: .legible)
            guard !Task.isCancelled, player?.currentItem === item else { return }
            audioGroup = audio
            subtitleGroup = subtitles
            if let audio {
                let matching = audioTrackPreference.flatMap { preference in audio.options.first { trackPreferenceKey($0) == preference } }
                    ?? audio.options.first { !settings.audioLanguage.isEmpty && $0.extendedLanguageTag?.lowercased().hasPrefix(settings.audioLanguage.lowercased()) == true }
                if let matching {
                    item.select(matching, in: audio)
                    selectedAudio = matching
                } else {
                    selectedAudio = item.currentMediaSelection.selectedMediaOption(in: audio)
                }
                if audioTrackPreference == nil, let selectedAudio {
                    audioTrackPreference = trackPreferenceKey(selectedAudio)
                }
            }
            if let subtitles {
                if (subtitleOffSelected || (!settings.subtitlesEnabled && subtitleTrackPreference == nil)), subtitles.allowsEmptySelection {
                    item.select(nil, in: subtitles)
                    selectedSubtitle = nil
                } else if let matching = subtitleTrackPreference.flatMap({ preference in subtitles.options.first { trackPreferenceKey($0) == preference } })
                            ?? subtitles.options.first(where: { settings.subtitlesEnabled && !settings.subtitleLanguage.isEmpty && $0.extendedLanguageTag?.lowercased().hasPrefix(settings.subtitleLanguage.lowercased()) == true })
                            ?? (settings.subtitlesEnabled ? (item.currentMediaSelection.selectedMediaOption(in: subtitles) ?? subtitles.options.first) : nil) {
                    item.select(matching, in: subtitles)
                    selectedSubtitle = matching
                } else {
                    selectedSubtitle = item.currentMediaSelection.selectedMediaOption(in: subtitles)
                    if subtitleTrackPreference == nil, let selectedSubtitle {
                        subtitleTrackPreference = trackPreferenceKey(selectedSubtitle)
                    }
                }
            }
            loadingTracks = false
        }
    }

    private func captureTrackPreferences(from item: AVPlayerItem?) {
        guard let item else { return }
        if let audioGroup, let option = item.currentMediaSelection.selectedMediaOption(in: audioGroup) {
            audioTrackPreference = trackPreferenceKey(option)
        }
        if let subtitleGroup {
            if let option = item.currentMediaSelection.selectedMediaOption(in: subtitleGroup) {
                subtitleTrackPreference = trackPreferenceKey(option)
                subtitleOffSelected = false
            } else if subtitleGroup.allowsEmptySelection {
                subtitleTrackPreference = nil
                subtitleOffSelected = true
            }
        }
    }

    private func trackPreferenceKey(_ option: AVMediaSelectionOption) -> String {
        "\(option.locale?.identifier ?? "")|\(option.displayName.lowercased())"
    }

    private func dismiss() {
        cleanup()
        onDismiss()
    }

    private func cleanup(invalidateRequest: Bool = true, abandonPreparation: Bool = true) {
        timing?.record(.closed)
        let qualityUpgradeToSettle = automaticUpgradeTask
        qualityUpgradeToSettle?.cancel()
        // cleanup can run both from Close and onDisappear. Once handed to the
        // player, this is the active session, not an unused speculative source.
        if abandonPreparation, !acceptedPreparation { preparedEpisode?.cancel() }
        nextEpisodeTask?.cancel()
        nextEpisodeTask = nil
        nextCandidateTask?.cancel()
        nextCandidateTask = nil
        nextCandidateToken = nil
        episodePromptTask?.cancel()
        episodePromptTask = nil
        episodePromptToken = nil
        segmentTask?.cancel()
        segmentTask = nil
        pendingEpisode = nil
        unlockHideTask?.cancel()
        unlockHideTask = nil
        finishAdjustment(immediately: true)
        gestureSettings.restoreBrightness()
        preparationTask?.cancel()
        preparationTask = nil
        trackTask?.cancel()
        trackTask = nil
        audioGroup = nil
        subtitleGroup = nil
        selectedAudio = nil
        selectedSubtitle = nil
        loadingTracks = false
        activePanel = nil
        feedbackTask?.cancel()
        feedbackTask = nil
        seekFeedback = nil
        playerStatusCancellable?.cancel()
        playerStatusCancellable = nil
        renewalTask?.cancel()
        renewalTask = nil
        recoveryTask?.cancel()
        recoveryTask = nil
        stallTask?.cancel()
        stallTask = nil
        playbackClockWatchdogTask?.cancel()
        playbackClockWatchdogTask = nil
        seekQueue.cancel()
        seekTimeoutTask?.cancel()
        seekTimeoutTask = nil
        seekTimeoutState.reset()
        playbackClockStall.reset()
        isSeeking = false
        player?.currentItem?.cancelPendingSeeks()
        if let timeObserver, let player { player.removeTimeObserver(timeObserver) }
        timeObserver = nil
        if let endObserver { NotificationCenter.default.removeObserver(endObserver) }
        endObserver = nil
        if let accessLogObserver { NotificationCenter.default.removeObserver(accessLogObserver) }
        accessLogObserver = nil
        let finishingSource = authorizedSource
        authorizedSource = nil
        if invalidateRequest { preparedPlayback.invalidate(request: request) }
        itemStatusCancellable?.cancel()
        itemStatusCancellable = nil
        hideControlsTask?.cancel()
        player?.pause()
        progressTracker?.stop()
        if let finishingSource, sessionFinalizationTask == nil {
            if let qualityUpgradeToSettle {
                sessionFinalizationTask = Task {
                    await qualityUpgradeToSettle.value
                    PlaybackSessionFinalizer.shared.finish(finishingSource, request: request)
                }
            } else {
                PlaybackSessionFinalizer.shared.finish(finishingSource, request: request)
            }
        }
        progressTracker = nil
        player = nil
    }

    private func formatSeconds(_ totalSeconds: Double) -> String {
        guard !totalSeconds.isNaN, !totalSeconds.isInfinite, totalSeconds >= 0 else { return "0:00" }
        let total = Int(totalSeconds)
        let hours = total / 3600
        let minutes = (total % 3600) / 60
        let seconds = total % 60
        if hours > 0 {
            return String(format: "%d:%02d:%02d", hours, minutes, seconds)
        }
        return String(format: "%d:%02d", minutes, seconds)
    }
}

/// Native tracking keeps scrubbing immediate while the visual footprint stays
/// small. The containing row supplies the full 44-point touch height.
private struct PlayerSeekSlider: UIViewRepresentable {
    @Binding var value: Double
    let upperBound: Double
    let onEditingChanged: (Bool) -> Void
    @Environment(\.isEnabled) private var isEnabled

    func makeCoordinator() -> Coordinator { Coordinator(parent: self) }

    func makeUIView(context: Context) -> SlimSeekSlider {
        let slider = SlimSeekSlider()
        let thumb = UIGraphicsImageRenderer(size: CGSize(width: 12, height: 12)).image { context in
            UIColor.white.setFill()
            context.cgContext.fillEllipse(in: CGRect(x: 0, y: 0, width: 12, height: 12))
        }
        slider.setThumbImage(thumb, for: .normal)
        slider.setThumbImage(thumb, for: .highlighted)
        slider.minimumTrackTintColor = UIColor(MelloTVTheme.accent)
        slider.maximumTrackTintColor = UIColor.white.withAlphaComponent(0.22)
        slider.addTarget(context.coordinator, action: #selector(Coordinator.began), for: .touchDown)
        slider.addTarget(context.coordinator, action: #selector(Coordinator.changed(_:)), for: .valueChanged)
        slider.addTarget(context.coordinator, action: #selector(Coordinator.ended),
                         for: [.touchUpInside, .touchUpOutside, .touchCancel])
        // SwiftUI supplies the existing playback-position accessibility actions.
        slider.isAccessibilityElement = false
        return slider
    }

    func updateUIView(_ slider: SlimSeekSlider, context: Context) {
        context.coordinator.parent = self
        slider.minimumValue = 0
        slider.maximumValue = Float(upperBound)
        slider.isEnabled = isEnabled
        if !slider.isTracking { slider.setValue(Float(value), animated: false) }
    }

    final class Coordinator: NSObject {
        var parent: PlayerSeekSlider
        private var editing = false
        init(parent: PlayerSeekSlider) { self.parent = parent }

        @objc func began() {
            guard !editing else { return }
            editing = true
            parent.onEditingChanged(true)
        }

        @objc func changed(_ slider: UISlider) {
            began()
            parent.value = Double(slider.value)
        }

        @objc func ended() {
            guard editing else { return }
            editing = false
            parent.onEditingChanged(false)
        }
    }
}

private final class SlimSeekSlider: UISlider {
    override func trackRect(forBounds bounds: CGRect) -> CGRect {
        let track = super.trackRect(forBounds: bounds)
        return CGRect(x: track.minX, y: bounds.midY - 2, width: track.width, height: 4)
    }
}

private enum PlaybackPanel: String, Identifiable {
    case tracks, speed, quality, episodes
    var id: String { rawValue }
}

@MainActor
private struct PlaybackEpisodesSheet: View {
    let request: PlaybackRequest
    let playingProgress: Double
    let onSelect: (Int, Episode) -> Void
    @Environment(\.dismiss) private var dismiss
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @State private var selectedSeason: Int
    @State private var seasons: [SeasonSummary] = []
    @State private var episodes: [Episode] = []
    @State private var isLoading = true
    @State private var episodeError: String?
    @State private var seasonError: String?
    @State private var retry = 0
    @State private var seasonRetry = 0
    @State private var savedProgress: [String: Double] = [:]
    @State private var loadedProgress = Set<String>()
    private let api = MelloTVAPI(baseURL: ServerAddress.baseURL)

    init(request: PlaybackRequest, playingProgress: Double, onSelect: @escaping (Int, Episode) -> Void) {
        self.request = request
        self.playingProgress = playingProgress
        self.onSelect = onSelect
        // The sheet starts at the playing season; subsequent changes are local.
        _selectedSeason = State(initialValue: request.season ?? 1)
    }

    var body: some View {
        GeometryReader { geometry in
            VStack(spacing: 0) {
                browserHeader
                    .padding(.horizontal, 24)
                    .padding(.top, 8)
                    .padding(.bottom, 8)

                // Vertical overflow remains available on short displays and at
                // larger text sizes; the episode shelf itself scrolls sideways.
                GeometryReader { contentGeometry in
                ScrollView(.vertical) {
                    VStack(alignment: .leading, spacing: 16) {
                        if let seasonError {
                            HStack(spacing: 12) {
                                Text(seasonError).font(.subheadline).foregroundStyle(MelloTVTheme.muted)
                                Button("Reload") { seasonRetry += 1 }
                                    .font(.subheadline.weight(.semibold))
                                    .touchTarget()
                            }
                            .padding(.horizontal, 24)
                        }
                        if isLoading {
                            ProgressView("Loading episodes")
                                .tint(.white)
                                .frame(maxWidth: .infinity, minHeight: 160)
                        } else if let episodeError {
                            VStack(spacing: 12) {
                                Text(episodeError)
                                    .foregroundStyle(MelloTVTheme.textSecondary)
                                    .multilineTextAlignment(.center)
                                Button("Try again") { retry += 1 }
                                    .font(.subheadline.weight(.semibold))
                                    .touchTarget()
                            }
                            .frame(maxWidth: .infinity, minHeight: 160)
                            .padding(.horizontal, 24)
                        } else if episodes.isEmpty {
                            Text("No episodes are available for this season.")
                                .foregroundStyle(MelloTVTheme.muted)
                                .frame(maxWidth: .infinity, minHeight: 160)
                        } else {
                            episodeShelf(width: geometry.size.width)
                        }
                    }
                    // Center the complete preview-and-copy group. Never stretch
                    // individual cards: surplus space belongs outside the shelf.
                    .padding(.vertical, 16)
                    .frame(minHeight: contentGeometry.size.height, alignment: .center)
                }
                .scrollIndicators(.hidden)
                }
            }
            .background(MelloTVTheme.background)
        }
        .foregroundStyle(.white)
        .tint(.white)
        .preferredColorScheme(.dark)
        .presentationDetents([.large])
        .presentationDragIndicator(.hidden)
        .task(id: seasonRetry) { await loadSeasons() }
        .task(id: "\(selectedSeason)-\(retry)") { await loadEpisodes() }
    }

    private var browserHeader: some View {
        HStack(spacing: 20) {
            Menu {
                ForEach(seasons) { season in
                    Button {
                        selectedSeason = season.number
                    } label: {
                        if season.number == selectedSeason {
                            Label(seasonTitle(season), systemImage: "checkmark")
                        } else {
                            Text(seasonTitle(season))
                        }
                    }
                }
            } label: {
                HStack(spacing: 14) {
                    Text(seasons.first(where: { $0.number == selectedSeason }).map(seasonTitle)
                         ?? "Season \(selectedSeason)")
                        .font(.headline)
                        .lineLimit(1)
                    PhosphorIcon("caret-down", size: 14, weight: .bold)
                }
                .padding(.horizontal, 18)
                .frame(minHeight: 48)
                .playerControlSurface(in: Capsule())
                .contentShape(Rectangle())
            }
            .disabled(seasons.isEmpty)
            .accessibilityLabel("Choose season, currently season \(selectedSeason)")
            Spacer(minLength: 8)
            Text(request.title)
                .font(.subheadline.weight(.medium))
                .foregroundStyle(MelloTVTheme.muted)
                .lineLimit(1)
            Button { dismiss() } label: {
                PhosphorIcon("x", size: 16, weight: .bold)
                    .frame(width: 48, height: 48)
                    .playerControlSurface(in: Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Close episodes")
        }
    }

    private func seasonTitle(_ season: SeasonSummary) -> String {
        season.name.isEmpty ? "Season \(season.number)" : season.name
    }

    private func episodeShelf(width: CGFloat) -> some View {
        // Around three previews in landscape with a partial next card to signal
        // scrolling. Match the approved guide with a compact title row.
        let cardWidth = min(300, max(232, (width - 48) / 3.2))
        return ScrollViewReader { proxy in
            ScrollView(.horizontal) {
                LazyHStack(alignment: .top, spacing: 20) {
                    ForEach(episodes) { episode in
                        episodeCard(episode, width: cardWidth)
                            .id(episode.number)
                    }
                }
                .scrollTargetLayout()
                .padding(.horizontal, 24)
            }
            .scrollIndicators(.hidden)
            .scrollTargetBehavior(.viewAligned)
            .task(id: episodes.map(\.number)) {
                await Task.yield()
                guard !Task.isCancelled else { return }
                let target = selectedSeason == request.season ? request.episode : episodes.first?.number
                if let target, episodes.contains(where: { $0.number == target }) {
                    proxy.scrollTo(target, anchor: .leading)
                }
            }
        }
    }

    private func episodeCard(_ episode: Episode, width: CGFloat) -> some View {
        let progressKey = "\(selectedSeason)-\(episode.number)"
        let isCurrent = selectedSeason == request.season && episode.number == request.episode
        let progress = isCurrent ? playingProgress : savedProgress[progressKey]
        return Button {
            if isCurrent { dismiss() } else { onSelect(selectedSeason, episode) }
        } label: {
            VStack(alignment: .leading, spacing: 0) {
                ZStack {
                    AsyncImage(url: episode.stillURL) { image in
                        image.resizable().scaledToFill()
                    } placeholder: {
                        Rectangle().fill(MelloTVTheme.secondaryBackground)
                    }
                    .frame(width: width, height: width * 9 / 16)
                    .clipped()

                    if !isCurrent {
                        PhosphorIcon("play", size: 24, weight: .fill)
                            .frame(width: 48, height: 48)
                            .background(.black.opacity(0.6), in: Circle())
                    }
                }
                .overlay(alignment: .bottomLeading) {
                    VStack(alignment: .leading, spacing: 8) {
                        if isCurrent {
                            Text("Playing")
                                .font(.caption.weight(.semibold))
                                .padding(.horizontal, 10)
                                .padding(.vertical, 5)
                                .background(.black.opacity(0.72), in: Capsule())
                                .padding(.leading, 10)
                        }
                        if let progress, progress > 0 {
                            ZStack(alignment: .leading) {
                                Rectangle().fill(.black.opacity(0.55))
                                Rectangle()
                                    .fill(MelloTVTheme.accent)
                                    .frame(width: width * min(1, max(0, progress)))
                            }
                            .frame(height: 4)
                        }
                    }
                    .allowsHitTesting(false)
                }
                .artworkCardSurface()
                .overlay {
                    if isCurrent {
                        RoundedRectangle(cornerRadius: 16)
                            .strokeBorder(MelloTVTheme.accent, lineWidth: 1.5)
                    }
                }

                Text("\(episode.number). \(episode.name)")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(isCurrent ? .white : MelloTVTheme.textSecondary)
                    .lineLimit(dynamicTypeSize.isAccessibilitySize ? 2 : 1, reservesSpace: true)
                    .multilineTextAlignment(.leading)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.top, 12)

                VStack(alignment: .leading, spacing: 6) {
                    Text(episode.runtime.map { "\($0) min" } ?? "Episode \(episode.number)")
                        .font(.caption)
                        .foregroundStyle(MelloTVTheme.muted)
                    Text(episode.overview.isEmpty ? "No synopsis available." : episode.overview)
                        .font(.subheadline)
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .lineSpacing(4)
                        .lineLimit(4, reservesSpace: true)
                        .multilineTextAlignment(.leading)
                }
                .padding(.top, 8)
            }
            .frame(width: width, alignment: .leading)
            .fixedSize(horizontal: false, vertical: true)
            .contentShape(Rectangle())
        }
        .buttonStyle(CardScaleButtonStyle(scale: 0.98))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Episode \(episode.number), \(episode.name)\(isCurrent ? ", currently playing" : "")")
        .accessibilityValue(progress.map { "\(Int(($0 * 100).rounded())) percent played. \(episode.overview)" }
                             ?? episode.overview)
        .task(id: progressKey) {
            guard !isCurrent else { return }
            await loadProgress(for: episode.number, season: selectedSeason)
        }
        .accessibilityHint(isCurrent ? "Return to the current video" : "Play this episode")
    }

    private func loadProgress(for episode: Int, season: Int) async {
        let key = "\(season)-\(episode)"
        guard !loadedProgress.contains(key) else { return }
        do {
            let fraction = try await api.episodeProgress(profileID: request.profileID,
                titleID: request.titleID, season: season, episode: episode)
            guard !Task.isCancelled, selectedSeason == season else { return }
            if let fraction { savedProgress[key] = fraction }
            loadedProgress.insert(key)
        } catch {
            // Unknown/unavailable history is not zero progress. Keep the card
            // playable; a later appearance can retry this optional exact read.
        }
    }

    private func loadSeasons() async {
        seasonError = nil
        do {
            let details = try await api.details(for: request.catalogTitle)
            try Task.checkCancellation()
            var loaded = details.seasons.filter { $0.episodeCount > 0 || $0.number == selectedSeason }
            if !loaded.contains(where: { $0.number == selectedSeason }) {
                loaded.append(SeasonSummary(number: selectedSeason, name: "Season \(selectedSeason)", episodeCount: 0))
            }
            seasons = loaded.sorted { $0.number < $1.number }
        } catch {
            guard !Task.isCancelled else { return }
            seasonError = "Other seasons couldn’t be loaded. Your current season is still available."
        }
    }

    private func loadEpisodes() async {
        let season = selectedSeason
        isLoading = true
        episodeError = nil
        episodes = []
        do {
            let loaded = try await api.seasonEpisodes(tvID: request.titleID, season: season)
            guard !Task.isCancelled, selectedSeason == season else { return }
            episodes = loaded.sorted { $0.number < $1.number }
            isLoading = false
        } catch {
            guard !Task.isCancelled, selectedSeason == season else { return }
            episodeError = error.localizedDescription
            isLoading = false
        }
    }
}

// MARK: - AVPlayer UIViewControllerRepresentable

private struct VideoSurface: UIViewControllerRepresentable {
    let player: AVPlayer
    let gestureSettings: PlayerGestureSettings
    let timing: PlaybackTiming?
    var onReadyForDisplay: (() -> Void)? = nil

    func makeCoordinator() -> Coordinator {
        Coordinator(onReadyForDisplay: onReadyForDisplay)
    }

    final class Coordinator {
        var onReadyForDisplay: (() -> Void)?
        var observation: NSKeyValueObservation?

        init(onReadyForDisplay: (() -> Void)?) {
            self.onReadyForDisplay = onReadyForDisplay
        }
    }

    func makeUIViewController(context: Context) -> AVPlayerViewController {
        let controller = AVPlayerViewController()
        timing?.record(.surfaceAttached)
        controller.player = player
        controller.showsPlaybackControls = false
        controller.videoGravity = .resizeAspect
        controller.view.backgroundColor = .black
        gestureSettings.attach(to: controller.view)
        let coordinator = context.coordinator
        let trace = timing
        coordinator.observation = controller.observe(\.isReadyForDisplay, options: [.initial, .new]) { [weak coordinator] controller, _ in
            guard controller.isReadyForDisplay else { return }
            let size = controller.player?.currentItem?.presentationSize ?? .zero
            trace?.record(.firstFrameReadyForDisplay, width: Double(size.width), height: Double(size.height))
            DispatchQueue.main.async {
                coordinator?.onReadyForDisplay?()
            }
        }
        return controller
    }

    func updateUIViewController(_ uiViewController: AVPlayerViewController, context: Context) {
        context.coordinator.onReadyForDisplay = onReadyForDisplay
        // Time/gesture updates must not rebind AVKit's unchanged player.
        if uiViewController.player !== player {
            uiViewController.player = player
        }
    }
}


// The touch surface sits above AVKit and below the SwiftUI controls. Pan
// recognition never waits for either tap recognizer to finish deciding.
private struct PlayerTouchSurface: UIViewRepresentable {
    let adjustmentsEnabled: Bool
    let onTap: () -> Void
    let onDoubleTap: (CGPoint, CGFloat) -> Void
    let onAdjustmentBegan: (PlayerAdjustment.Kind) -> Void
    let onAdjustmentChanged: (CGFloat, CGFloat) -> Void
    let onAdjustmentEnded: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeUIView(context: Context) -> UIView {
        let view = UIView()
        view.backgroundColor = .clear
        view.isMultipleTouchEnabled = false
        let coordinator = context.coordinator
        let pan = UIPanGestureRecognizer(target: coordinator, action: #selector(Coordinator.pan(_:)))
        pan.maximumNumberOfTouches = 1
        pan.delegate = coordinator
        pan.isEnabled = adjustmentsEnabled
        let single = UITapGestureRecognizer(target: coordinator, action: #selector(Coordinator.singleTap(_:)))
        let double = UITapGestureRecognizer(target: coordinator, action: #selector(Coordinator.doubleTap(_:)))
        double.numberOfTapsRequired = 2
        single.require(toFail: double)
        single.require(toFail: pan)
        double.require(toFail: pan)
        view.addGestureRecognizer(pan)
        view.addGestureRecognizer(single)
        view.addGestureRecognizer(double)
        coordinator.panRecognizer = pan
        return view
    }

    func updateUIView(_ uiView: UIView, context: Context) {
        context.coordinator.parent = self
        if let pan = context.coordinator.panRecognizer, pan.isEnabled != adjustmentsEnabled {
            pan.isEnabled = adjustmentsEnabled
        }
    }

    static func dismantleUIView(_ uiView: UIView, coordinator: Coordinator) {
        coordinator.endPan()
    }

    @MainActor
    final class Coordinator: NSObject, UIGestureRecognizerDelegate {
        var parent: PlayerTouchSurface
        weak var panRecognizer: UIPanGestureRecognizer?
        private var activeKind: PlayerAdjustment.Kind?
        private var hasBegun = false
        private var lastSeekTap: TimeInterval = -.infinity
        private var lastSeekWasForward = false

        init(_ parent: PlayerTouchSurface) { self.parent = parent }

        func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
            guard let pan = gestureRecognizer as? UIPanGestureRecognizer,
                  parent.adjustmentsEnabled, let view = pan.view else { return false }
            let location = pan.location(in: view)
            let velocity = pan.velocity(in: view)
            guard abs(velocity.y) > abs(velocity.x), location.y > 72,
                  location.y < view.bounds.height - 100 else { return false }
            if location.x < view.bounds.width * 0.4 {
                activeKind = .brightness
            } else if location.x > view.bounds.width * 0.6 {
                activeKind = .volume
            } else {
                return false
            }
            return true
        }

        @objc func pan(_ recognizer: UIPanGestureRecognizer) {
            guard let view = recognizer.view else { return }
            switch recognizer.state {
            case .began:
                guard let activeKind else { return }
                hasBegun = true
                recognizer.setTranslation(.zero, in: view)
                parent.onAdjustmentBegan(activeKind)
            case .changed, .ended:
                guard hasBegun else { return }
                let delta = recognizer.translation(in: view).y
                recognizer.setTranslation(.zero, in: view)
                // A full-range sweep is deliberately longer than the pill.
                // The minimum also keeps landscape adjustments controllable.
                parent.onAdjustmentChanged(delta, max(360, view.bounds.height * 0.85))
                if recognizer.state == .ended { endPan() }
            case .cancelled, .failed:
                endPan()
            default:
                break
            }
        }

        func endPan() {
            guard hasBegun else { return }
            hasBegun = false
            activeKind = nil
            parent.onAdjustmentEnded()
        }

        @objc func singleTap(_ recognizer: UITapGestureRecognizer) {
            guard recognizer.state == .ended, let view = recognizer.view else { return }
            let location = recognizer.location(in: view)
            let now = ProcessInfo.processInfo.systemUptime
            // A trailing tap in a skip burst continues skipping instead of
            // unexpectedly revealing the complete playback overlay.
            if now - lastSeekTap < 0.75,
               (location.x >= view.bounds.width / 2) == lastSeekWasForward {
                lastSeekTap = now
                parent.onDoubleTap(location, view.bounds.width)
            } else {
                parent.onTap()
            }
        }

        @objc func doubleTap(_ recognizer: UITapGestureRecognizer) {
            guard recognizer.state == .ended, let view = recognizer.view else { return }
            let location = recognizer.location(in: view)
            lastSeekTap = ProcessInfo.processInfo.systemUptime
            lastSeekWasForward = location.x >= view.bounds.width / 2
            parent.onDoubleTap(location, view.bounds.width)
        }
    }
}

// Keep high-frequency level changes inside this small view. The video surface
// and full playback overlay do not need to redraw for every drag sample.
@MainActor
@Observable
private final class PlayerAdjustmentState {
    var adjustment: PlayerAdjustment?
    var isVisible = false
}

private struct PlayerAdjustmentIndicator: View {
    let state: PlayerAdjustmentState
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        ZStack {
            if state.isVisible, let adjustment = state.adjustment {
                HStack {
                    if adjustment.kind == .volume { Spacer() }
                    VStack(spacing: 10) {
                        ZStack(alignment: .bottom) {
                            Capsule().fill(.black.opacity(0.4))
                            Capsule().fill(.white.opacity(0.25))
                            Rectangle()
                                .fill(.white)
                                .frame(height: 140 * CGFloat(adjustment.value))
                        }
                        .frame(width: 20, height: 140)
                        .clipShape(Capsule())
                        // The fill tracks the finger directly; only visibility fades.
                        .transaction { transaction in
                            transaction.animation = nil
                            transaction.disablesAnimations = true
                        }

                        PhosphorIcon(
                            adjustment.kind == .brightness ? "sun" :
                                (adjustment.value == 0 ? "speaker-slash" : "speaker-high"),
                            size: 20,
                            weight: .fill
                        )
                        .foregroundStyle(.white)
                        .shadow(color: .black.opacity(0.6), radius: 2, y: 1)
                        .accessibilityHidden(true)
                    }
                    if adjustment.kind == .brightness { Spacer() }
                }
                .padding(.horizontal, 24)
                .transition(.opacity)
            }
        }
        .animation(reduceMotion ? nil : .easeOut(duration: 0.1), value: state.isVisible)
    }
}

private struct PlayerAdjustment {
    enum Kind { case brightness, volume }
    let kind: Kind
    var value: Float
}

@MainActor
private final class PlayerGestureSettings {
    private weak var hostView: UIView?
    private weak var brightnessScreen: UIScreen?
    private var originalBrightness: CGFloat?
    private var pendingSetting: (value: Float, kind: PlayerAdjustment.Kind)?
    private var displayLink: CADisplayLink?
    // MPVolumeView owns the system output slider and tracks hardware-button
    // changes. Keep it attached to the player's window for the gesture control.
    private let volumeView = MPVolumeView(frame: CGRect(x: -200, y: 0, width: 160, height: 40))

    func attach(to view: UIView) {
        hostView = view
        volumeView.alpha = 0.01
        volumeView.isUserInteractionEnabled = false
        volumeView.accessibilityElementsHidden = true
        view.addSubview(volumeView)
    }

    func initialValue(for kind: PlayerAdjustment.Kind) -> Float? {
        switch kind {
        case .brightness:
            guard let screen = hostView?.window?.windowScene?.screen else { return nil }
            if originalBrightness == nil {
                originalBrightness = screen.brightness
                brightnessScreen = screen
            }
            return Float(screen.brightness)
        case .volume:
            guard volumeSlider != nil else { return nil }
            return AVAudioSession.sharedInstance().outputVolume
        }
    }

    func schedule(_ value: Float, for kind: PlayerAdjustment.Kind) {
        pendingSetting = (min(1, max(0, value)), kind)
        if displayLink == nil {
            let link = CADisplayLink(target: FrameTarget(owner: self), selector: #selector(FrameTarget.tick))
            link.add(to: .main, forMode: .common)
            displayLink = link
        }
        displayLink?.isPaused = false
    }

    func finishUpdates() {
        applyPendingSetting()
    }

    func cancelPendingUpdates() {
        pendingSetting = nil
        displayLink?.invalidate()
        displayLink = nil
    }

    private func applyPendingSetting() {
        displayLink?.isPaused = true
        guard let setting = pendingSetting else { return }
        pendingSetting = nil
        _ = set(setting.value, for: setting.kind)
    }

    @MainActor
    private final class FrameTarget: NSObject {
        weak var owner: PlayerGestureSettings?
        init(owner: PlayerGestureSettings) { self.owner = owner }
        @objc func tick() { owner?.applyPendingSetting() }
    }

    private func set(_ value: Float, for kind: PlayerAdjustment.Kind) -> Float {
        let level = min(1, max(0, value))
        switch kind {
        case .brightness:
            guard let screen = brightnessScreen else { return level }
            screen.brightness = CGFloat(level)
            return level
        case .volume:
            guard let slider = volumeSlider else { return AVAudioSession.sharedInstance().outputVolume }
            slider.setValue(level, animated: false)
            slider.sendActions(for: .valueChanged)
            return slider.value
        }
    }

    func restoreBrightness() {
        cancelPendingUpdates()
        if let originalBrightness, let brightnessScreen {
            brightnessScreen.brightness = originalBrightness
        }
        originalBrightness = nil
        brightnessScreen = nil
    }

    private var volumeSlider: UISlider? {
        volumeView.subviews.compactMap { $0 as? UISlider }.first
    }
}

private extension View {
    func playerControlSurface<S: Shape>(in shape: S) -> some View {
        background(.black.opacity(0.65), in: shape)
    }
}
