import SwiftUI
import MelloKit

/// 10-foot tvOS Hero Showroom Carousel inspired by iOS's showroom stage.
/// Features a wide cinematic backdrop with showroom spotlight lighting, gradient contrast overlays,
/// dynamic metadata, rating, synopsis, and focusable Play and Details action buttons.
public struct FeaturedCarouselView: View {
    public let titles: [MediaTitle]
    public var onSelect: (MediaTitle) -> Void
    public var onPlay: (MediaTitle) -> Void

    @State private var currentIndex: Int = 0
    @State private var timer: Timer?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private let advanceInterval: TimeInterval = 9.0

    private var featuredTitles: [MediaTitle] {
        Array(titles.prefix(7))
    }

    private var currentTitle: MediaTitle? {
        guard !featuredTitles.isEmpty else { return nil }
        return featuredTitles[currentIndex % featuredTitles.count]
    }

    public init(
        titles: [MediaTitle],
        viewportSize: CGSize = .zero,
        onSelect: @escaping (MediaTitle) -> Void,
        onPlay: @escaping (MediaTitle) -> Void
    ) {
        self.titles = titles
        self.onSelect = onSelect
        self.onPlay = onPlay
    }

    public var body: some View {
        if let title = currentTitle {
            ZStack(alignment: .bottomLeading) {
                // Wide Cinematic Backdrop with Showroom Lighting
                backdropStage(title: title)

                // Leading Vignette & Bottom Blends
                LinearGradient(
                    stops: [
                        .init(color: Color.black.opacity(0.85), location: 0.0),
                        .init(color: Color.black.opacity(0.55), location: 0.45),
                        .init(color: Color.clear, location: 0.80)
                    ],
                    startPoint: .leading,
                    endPoint: .trailing
                )

                LinearGradient(
                    stops: [
                        .init(color: Color.clear, location: 0.4),
                        .init(color: Color.black.opacity(0.4), location: 0.7),
                        .init(color: Color.black.opacity(0.95), location: 1.0)
                    ],
                    startPoint: .top,
                    endPoint: .bottom
                )

                // Top Subtle Vignette for Tab Bar Clarity
                LinearGradient(
                    colors: [Color.black.opacity(0.6), Color.clear],
                    startPoint: .top,
                    endPoint: .center
                )

                // Content Overlay (Title, Metadata, Overview, Action Buttons)
                VStack(alignment: .leading, spacing: 14) {
                    // Eyebrow / Genres
                    if !title.genres.isEmpty {
                        Text(title.genres.prefix(3).joined(separator: " • ").uppercased())
                            .font(.caption.weight(.bold))
                            .tracking(2.0)
                            .foregroundStyle(MelloTVTheme.accent)
                    } else {
                        Text("MELLO FEATURED")
                            .font(.caption.weight(.bold))
                            .tracking(2.0)
                            .foregroundStyle(MelloTVTheme.accent)
                    }

                    // Main Title
                    Text(title.title)
                        .font(.system(size: 52, weight: .heavy, design: .rounded))
                        .foregroundStyle(Color.white)
                        .lineLimit(2)
                        .shadow(color: Color.black.opacity(0.8), radius: 8, x: 0, y: 4)

                    // Metadata Row (Year, Rating, Type Badge)
                    HStack(spacing: 16) {
                        if !title.year.isEmpty {
                            Text(title.year)
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(Color.white.opacity(0.85))
                        }

                        if title.rating > 0 {
                            HStack(spacing: 5) {
                                Image(systemName: "star.fill")
                                    .font(.system(size: 14))
                                    .foregroundStyle(Color.yellow)
                                Text(String(format: "%.1f", title.rating))
                                    .font(.subheadline.weight(.bold))
                                    .foregroundStyle(Color.white)
                            }
                        }

                        Text(title.type == .movie ? "Movie" : "TV Series")
                            .font(.caption.weight(.bold))
                            .foregroundStyle(Color.white)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 4)
                            .background(Color.white.opacity(0.22))
                            .clipShape(Capsule())
                    }

                    // Logline / Synopsis
                    if !title.overview.isEmpty {
                        Text(title.overview)
                            .font(.body)
                            .foregroundStyle(Color.white.opacity(0.88))
                            .lineLimit(3)
                            .frame(maxWidth: 720, alignment: .leading)
                            .shadow(color: Color.black.opacity(0.6), radius: 4, x: 0, y: 2)
                    }

                    // Focusable Action Buttons
                    HStack(spacing: 20) {
                        Button {
                            onPlay(title)
                        } label: {
                            HStack(spacing: 10) {
                                Image(systemName: "play.fill")
                                Text("Play")
                                    .fontWeight(.bold)
                            }
                            .padding(.horizontal, 26)
                            .padding(.vertical, 12)
                        }
                        .buttonStyle(.borderedProminent)
                        .tint(MelloTVTheme.accent)

                        Button {
                            onSelect(title)
                        } label: {
                            HStack(spacing: 8) {
                                Image(systemName: "info.circle")
                                Text("Details")
                                    .fontWeight(.semibold)
                            }
                            .padding(.horizontal, 22)
                            .padding(.vertical, 12)
                        }
                        .buttonStyle(.bordered)
                    }
                    .padding(.top, 10)
                }
                .padding(.leading, 64)
                .padding(.bottom, 48)

                // Showroom Carousel Indicator Pills
                if featuredTitles.count > 1 {
                    HStack(spacing: 10) {
                        ForEach(0..<featuredTitles.count, id: \.self) { idx in
                            Button {
                                withAnimation(reduceMotion ? .none : .easeInOut(duration: 0.4)) {
                                    currentIndex = idx
                                }
                                resetTimer()
                            } label: {
                                Capsule()
                                    .fill(idx == currentIndex ? MelloTVTheme.accent : Color.white.opacity(0.35))
                                    .frame(width: idx == currentIndex ? 36 : 10, height: 10)
                                    .animation(.spring(response: 0.35, dampingFraction: 0.75), value: currentIndex)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.trailing, 64)
                    .padding(.bottom, 48)
                    .frame(maxWidth: .infinity, alignment: .bottomTrailing)
                }
            }
            .frame(height: 580)
            .onAppear(perform: startTimer)
            .onDisappear(perform: stopTimer)
        }
    }

    // MARK: - Backdrop Showroom Stage

    @ViewBuilder
    private func backdropStage(title: MediaTitle) -> some View {
        ZStack {
            // Showroom floor spotlight aura centered beneath the backdrop
            EllipticalGradient(
                colors: [
                    MelloTVTheme.accent.opacity(0.14),
                    Color.black.opacity(0.10),
                    Color.clear
                ],
                center: .center,
                startRadiusFraction: 0.05,
                endRadiusFraction: 0.75
            )
            .frame(maxWidth: .infinity, maxHeight: 580)

            // Backdrop Image with crossfade transition
            if let imageURL = title.backdropURL ?? title.posterURL {
                AsyncImage(url: imageURL) { phase in
                    switch phase {
                    case .success(let image):
                        image
                            .resizable()
                            .aspectRatio(contentMode: .fill)
                            .frame(maxWidth: .infinity, maxHeight: 580)
                            .clipped()
                            .id(title.id)
                            .transition(.opacity.animation(.easeInOut(duration: 0.45)))
                    case .empty, .failure:
                        MelloTVTheme.secondaryBackground
                    @unknown default:
                        MelloTVTheme.secondaryBackground
                    }
                }
            } else {
                MelloTVTheme.secondaryBackground
            }
        }
        .frame(height: 580)
    }

    // MARK: - Carousel Auto-Advance Timer

    private func startTimer() {
        stopTimer()
        guard featuredTitles.count > 1, !reduceMotion else { return }
        timer = Timer.scheduledTimer(withTimeInterval: advanceInterval, repeats: true) { _ in
            Task { @MainActor in
                withAnimation(reduceMotion ? .none : .easeInOut(duration: 0.5)) {
                    currentIndex = (currentIndex + 1) % featuredTitles.count
                }
            }
        }
    }

    private func stopTimer() {
        timer?.invalidate()
        timer = nil
    }

    private func resetTimer() {
        startTimer()
    }
}
