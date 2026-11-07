import SwiftUI
import MelloKit

/// 10-foot tvOS Hero Showroom Carousel conforming to Apple TV Human Interface Guidelines.
/// Features a wide cinematic 740pt backdrop with multi-angle contrast vignetting,
/// smooth cross-fade slide transitions keyed by ID, 10-foot legible typography,
/// and native focusable Play and Details action buttons.
public struct FeaturedCarouselView: View {
    public let titles: [MediaTitle]
    public var onSelect: (MediaTitle) -> Void
    public var onPlay: (MediaTitle) -> Void

    @State private var currentIndex: Int = 0
    @State private var timer: Timer?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private let advanceInterval: TimeInterval = 9.0
    private let showroomHeight: CGFloat = 740

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
        ZStack(alignment: .bottomLeading) {
            if let title = currentTitle {
                heroSlide(title: title)
                    .id(title.id)
                    .transition(.opacity)
            }

            // Showroom Carousel Indicator Pills (Bottom Right)
            if featuredTitles.count > 1 {
                HStack(spacing: 8) {
                    ForEach(0..<featuredTitles.count, id: \.self) { idx in
                        Button {
                            withAnimation(reduceMotion ? .none : .easeInOut(duration: 0.4)) {
                                currentIndex = idx
                            }
                            resetTimer()
                        } label: {
                            Capsule()
                                .fill(idx == currentIndex ? Color.white : Color.white.opacity(0.35))
                                .frame(width: idx == currentIndex ? 32 : 8, height: 6)
                                .animation(.spring(response: 0.35, dampingFraction: 0.75), value: currentIndex)
                        }
                        .buttonStyle(.plain)
                        .focusable(false)
                    }
                }
                .padding(.trailing, 64)
                .padding(.bottom, 54)
                .frame(maxWidth: .infinity, alignment: .bottomTrailing)
            }
        }
        .frame(height: showroomHeight)
        .onAppear(perform: startTimer)
        .onDisappear(perform: stopTimer)
    }

    // MARK: - Hero Slide

    @ViewBuilder
    private func heroSlide(title: MediaTitle) -> some View {
        ZStack(alignment: .bottomLeading) {
            // 1. Cinematic Backdrop Image
            backdropStage(title: title)

            // 2. Top Vignette: Shields top tab navigation for clarity
            LinearGradient(
                stops: [
                    .init(color: Color.black.opacity(0.85), location: 0.0),
                    .init(color: Color.black.opacity(0.40), location: 0.18),
                    .init(color: Color.clear, location: 0.35)
                ],
                startPoint: .top,
                endPoint: .bottom
            )

            // 3. Leading Vignette: Deep contrast for metadata & logline
            LinearGradient(
                stops: [
                    .init(color: Color.black.opacity(0.92), location: 0.0),
                    .init(color: Color.black.opacity(0.70), location: 0.35),
                    .init(color: Color.black.opacity(0.25), location: 0.65),
                    .init(color: Color.clear, location: 0.85)
                ],
                startPoint: .leading,
                endPoint: .trailing
            )

            // 4. Bottom Vignette: Melts seamlessly into obsidian canvas
            LinearGradient(
                stops: [
                    .init(color: Color.clear, location: 0.40),
                    .init(color: Color.black.opacity(0.55), location: 0.70),
                    .init(color: MelloTVTheme.background, location: 1.0)
                ],
                startPoint: .top,
                endPoint: .bottom
            )

            // 5. Content Information Stage
            VStack(alignment: .leading, spacing: 14) {
                Spacer()

                // Eyebrow / Genre Pills
                if !title.genres.isEmpty {
                    Text(title.genres.prefix(3).joined(separator: "  •  ").uppercased())
                        .font(.system(size: 13, weight: .bold))
                        .tracking(2.5)
                        .foregroundStyle(MelloTVTheme.accent)
                } else {
                    Text("MELLO FEATURED")
                        .font(.system(size: 13, weight: .bold))
                        .tracking(2.5)
                        .foregroundStyle(MelloTVTheme.accent)
                }

                // Main Title
                Text(title.title)
                    .font(.system(size: 54, weight: .heavy, design: .rounded))
                    .foregroundStyle(Color.white)
                    .lineLimit(2)
                    .shadow(color: Color.black.opacity(0.8), radius: 8, x: 0, y: 4)

                // Metadata Row (Year, Rating, Type Badge)
                HStack(spacing: 16) {
                    if !title.year.isEmpty {
                        Text(title.year)
                            .font(.system(size: 17, weight: .semibold))
                            .foregroundStyle(Color.white.opacity(0.85))
                    }

                    if title.rating > 0 {
                        HStack(spacing: 5) {
                            Image(systemName: "star.fill")
                                .font(.system(size: 13))
                                .foregroundStyle(Color.yellow)
                            Text(String(format: "%.1f", title.rating))
                                .font(.system(size: 16, weight: .bold))
                                .foregroundStyle(Color.white)
                        }
                    }

                    Text(title.type == .movie ? "Movie" : "TV Series")
                        .font(.system(size: 12, weight: .bold))
                        .foregroundStyle(Color.white.opacity(0.9))
                        .padding(.horizontal, 10)
                        .padding(.vertical, 4)
                        .background(Color.white.opacity(0.18))
                        .clipShape(Capsule())
                }

                // Logline / Synopsis
                if !title.overview.isEmpty {
                    Text(title.overview)
                        .font(.system(size: 17, weight: .regular))
                        .lineSpacing(4)
                        .foregroundStyle(Color.white.opacity(0.85))
                        .lineLimit(3)
                        .frame(maxWidth: 820, alignment: .leading)
                        .shadow(color: Color.black.opacity(0.6), radius: 4, x: 0, y: 2)
                }

                // Focusable Action Buttons
                HStack(spacing: 20) {
                    Button {
                        onPlay(title)
                    } label: {
                        HStack(spacing: 10) {
                            Image(systemName: "play.fill")
                                .font(.system(size: 16, weight: .bold))
                            Text("Play")
                                .font(.system(size: 18, weight: .bold))
                        }
                        .padding(.horizontal, 28)
                        .padding(.vertical, 14)
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(Color.white)
                    .foregroundColor(Color.black)

                    Button {
                        onSelect(title)
                    } label: {
                        HStack(spacing: 8) {
                            Image(systemName: "info.circle")
                                .font(.system(size: 16, weight: .medium))
                            Text("Details")
                                .font(.system(size: 18, weight: .semibold))
                        }
                        .padding(.horizontal, 24)
                        .padding(.vertical, 14)
                    }
                    .buttonStyle(.bordered)
                }
                .padding(.top, 8)
            }
            .padding(.leading, 64)
            .padding(.bottom, 54)
            .padding(.top, 140) // Clearance for top tab bar
        }
        .frame(height: showroomHeight)
    }

    // MARK: - Backdrop Showroom Stage

    @ViewBuilder
    private func backdropStage(title: MediaTitle) -> some View {
        ZStack {
            if let imageURL = title.backdropURL ?? title.posterURL {
                AsyncImage(url: imageURL) { phase in
                    switch phase {
                    case .success(let image):
                        image
                            .resizable()
                            .aspectRatio(contentMode: .fill)
                            .frame(maxWidth: .infinity, maxHeight: showroomHeight)
                            .clipped()
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
        .frame(height: showroomHeight)
    }

    // MARK: - Carousel Auto-Advance Timer

    private func startTimer() {
        stopTimer()
        guard featuredTitles.count > 1, !reduceMotion else { return }
        timer = Timer.scheduledTimer(withTimeInterval: advanceInterval, repeats: true) { _ in
            Task { @MainActor in
                withAnimation(reduceMotion ? .none : .easeInOut(duration: 0.6)) {
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
