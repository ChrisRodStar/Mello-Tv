import SwiftUI
import MelloKit

/// 3D Showroom Stage Hero Carousel ported directly from Mello TV iOS.
/// Four visible depth layers with gentle, centered 3D perspective, graduated lighting,
/// dynamic poster aura, and the signature curved timeline arc indicator.
public struct FeaturedCarouselView: View {
    public let titles: [MediaTitle]
    public var onSelect: (MediaTitle) -> Void
    public var onPlay: (MediaTitle) -> Void

    @State private var position: Int = 0
    @State private var timer: Timer?
    @State private var advanceStartedAt: Date?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @FocusState private var isCenterFocused: Bool

    private let advanceInterval: TimeInterval = 8.0
    private let cardWidth: CGFloat = 340
    private let cardHeight: CGFloat = 510
    private let stepX: CGFloat = 240
    private let stageHeight: CGFloat = 620

    private var featuredTitles: [MediaTitle] {
        Array(titles.prefix(7))
    }

    private var currentIndex: Int {
        let count = featuredTitles.count
        guard count > 0 else { return 0 }
        return ((position % count) + count) % count
    }

    private var currentTitle: MediaTitle? {
        guard !featuredTitles.isEmpty else { return nil }
        return featuredTitles[currentIndex]
    }

    private var visibleSlots: [Int] {
        featuredTitles.count > 1 ? Array((position - 4)...(position + 4)) : [position]
    }

    private var settleAnimation: Animation {
        reduceMotion ? .easeOut(duration: 0.20) : .spring(response: 0.45, dampingFraction: 0.85)
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
        if !featuredTitles.isEmpty {
            VStack(spacing: 20) {
                // 3D Showroom Stage
                ZStack {
                    // Showroom floor spotlight aura centered beneath the stage
                    EllipticalGradient(
                        colors: [
                            MelloTVTheme.accent.opacity(0.12),
                            Color.black.opacity(0.06),
                            Color.clear
                        ],
                        center: .center,
                        startRadiusFraction: 0.05,
                        endRadiusFraction: 0.75
                    )
                    .frame(width: 1200, height: cardHeight * 0.95)
                    .offset(y: 30)
                    .allowsHitTesting(false)

                    // Staged 3D Cards
                    ForEach(visibleSlots, id: \.self) { slot in
                        stageCard(
                            title: featuredTitles[titleIndex(for: slot)],
                            slot: slot
                        )
                    }
                }
                .frame(maxWidth: .infinity)
                .frame(height: stageHeight)

                // Arc Page Indicator (Timeline progress)
                pageIndicator(span: 520)

                // Active Title Headline & Actions Bar
                if let title = currentTitle {
                    VStack(spacing: 8) {
                        Text(title.title)
                            .font(.system(size: 32, weight: .bold, design: .rounded))
                            .foregroundStyle(Color.white)
                            .lineLimit(1)
                            .shadow(color: Color.black.opacity(0.8), radius: 6, y: 3)

                        HStack(spacing: 12) {
                            if !title.year.isEmpty {
                                Text(title.year)
                                    .font(.system(size: 15, weight: .medium))
                                    .foregroundStyle(Color.white.opacity(0.75))
                            }

                            if title.rating > 0 {
                                HStack(spacing: 4) {
                                    Image(systemName: "star.fill")
                                        .font(.system(size: 11))
                                        .foregroundStyle(Color.yellow)
                                    Text(String(format: "%.1f", title.rating))
                                        .font(.system(size: 14, weight: .bold))
                                        .foregroundStyle(Color.white)
                                }
                            }

                            Text(title.type == .movie ? "Movie" : "TV Series")
                                .font(.system(size: 11, weight: .bold))
                                .foregroundStyle(Color.white.opacity(0.85))
                                .padding(.horizontal, 8)
                                .padding(.vertical, 3)
                                .background(Color.white.opacity(0.15))
                                .clipShape(Capsule())
                        }
                    }
                    .padding(.top, 4)
                    .transition(.opacity)
                    .id("meta-\(title.id)")
                }
            }
            .padding(.top, 24)
            .padding(.bottom, 16)
            .onAppear { startTimer() }
            .onDisappear { stopTimer() }
        }
    }

    private func titleIndex(for slot: Int) -> Int {
        let count = featuredTitles.count
        guard count > 0 else { return 0 }
        return ((slot % count) + count) % count
    }

    // MARK: - 3D Showroom Card

    @ViewBuilder
    private func stageCard(title: MediaTitle, slot: Int) -> some View {
        let effectiveOffset = CGFloat(slot - position)
        let absOffset = abs(effectiveOffset)
        let depth = min(absOffset, 3.5)
        let isCenter = absOffset < 0.35

        let xOffset = effectiveOffset * stepX
        let scale = 1.0 - depth * 0.09
        let zIndex = 100.0 - Double(absOffset) * 20.0
        let rotationY = Double(effectiveOffset) * 4.5
        let yOffset = depth * 4.0
        let opacity = Double(max(0, 1 - max(0, absOffset - 3.2) * 1.8))
        let focus = max(0.0, 1.0 - Double(absOffset))

        Button {
            if isCenter {
                onPlay(title)
            } else if effectiveOffset < 0 {
                withAnimation(settleAnimation) { position -= 1 }
                startTimer()
            } else {
                withAnimation(settleAnimation) { position += 1 }
                startTimer()
            }
        } label: {
            ZStack(alignment: .bottomTrailing) {
                // Card Artwork
                cardArtwork(title: title, width: cardWidth, height: cardHeight)
                    .blur(radius: depth * 0.6)

                // Depth Shadow Gradients
                LinearGradient(
                    colors: [
                        Color.black.opacity(Double(depth) * 0.08),
                        Color.black.opacity(Double(depth) * 0.18)
                    ],
                    startPoint: .top,
                    endPoint: .bottom
                )

                // Top Right Brand TV Mark
                VStack {
                    HStack {
                        Spacer()
                        BrandTVMark()
                            .padding(14)
                            .opacity(isCenter ? 1.0 : 0.0)
                    }
                    Spacer()
                }

                // Bottom Right Details Button
                if isCenter {
                    Button {
                        onSelect(title)
                    } label: {
                        CardDetailsGlyph()
                    }
                    .buttonStyle(.plain)
                    .padding(16)
                }
            }
            .frame(width: cardWidth, height: cardHeight)
            .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 22, style: .continuous)
                    .strokeBorder(
                        LinearGradient(
                            colors: (isCenter && isCenterFocused) ? [
                                Color.white.opacity(0.85),
                                Color.white.opacity(0.30)
                            ] : [
                                Color.white.opacity(0.12),
                                Color.white.opacity(0.03)
                            ],
                            startPoint: .top,
                            endPoint: .bottom
                        ),
                        lineWidth: (isCenter && isCenterFocused) ? 2.0 : 0.75
                    )
            )
            .background {
                if isCenter {
                    posterLight(title: title, width: cardWidth, height: cardHeight)
                        .opacity(focus)
                }
            }
            .shadow(
                color: Color.black.opacity(isCenter ? (isCenterFocused ? 0.70 : 0.45) : 0.20),
                radius: isCenter ? (isCenterFocused ? 32 : 20) : 8,
                x: 0,
                y: isCenter ? (isCenterFocused ? 18 : 10) : 4
            )
        }
        .buttonStyle(.plain)
        .focused($isCenterFocused, equals: isCenter)
        .onMoveCommand { direction in
            switch direction {
            case .left:
                withAnimation(settleAnimation) { position -= 1 }
                startTimer()
            case .right:
                withAnimation(settleAnimation) { position += 1 }
                startTimer()
            default:
                break
            }
        }
        .scaleEffect(isCenter && isCenterFocused ? 1.05 : scale)
        .rotation3DEffect(
            .degrees(reduceMotion ? 0 : rotationY),
            axis: (x: 0, y: 1, z: 0),
            anchor: .center,
            perspective: 0.35
        )
        .offset(x: xOffset, y: yOffset)
        .zIndex(zIndex)
        .opacity(opacity)
    }

    // MARK: - Diffuse Poster Aura

    @ViewBuilder
    private func posterLight(title: MediaTitle, width: CGFloat, height: CGFloat) -> some View {
        cardArtwork(title: title, width: width + 24, height: height + 24)
            .saturation(1.4)
            .brightness(0.15)
            .blur(radius: 24, opaque: false)
            .mask {
                RoundedRectangle(cornerRadius: 32, style: .continuous)
                    .strokeBorder(lineWidth: 18)
            }
            .frame(width: width, height: height)
    }

    // MARK: - Card Artwork

    @ViewBuilder
    private func cardArtwork(title: MediaTitle, width: CGFloat, height: CGFloat) -> some View {
        let imageURL = title.posterURL ?? title.backdropURL
        if let imageURL {
            AsyncImage(url: imageURL) { phase in
                switch phase {
                case .success(let image):
                    image
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                        .frame(width: width, height: height)
                        .clipped()
                case .empty, .failure:
                    artworkPlaceholder(title: title, width: width, height: height)
                @unknown default:
                    artworkPlaceholder(title: title, width: width, height: height)
                }
            }
        } else {
            artworkPlaceholder(title: title, width: width, height: height)
        }
    }

    private func artworkPlaceholder(title: MediaTitle, width: CGFloat, height: CGFloat) -> some View {
        ZStack {
            MelloTVTheme.secondaryBackground
            VStack(spacing: 12) {
                Image(systemName: "film")
                    .font(.system(size: 40))
                    .foregroundStyle(MelloTVTheme.muted.opacity(0.6))
                Text(title.title)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(MelloTVTheme.muted)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 12)
                    .lineLimit(2)
            }
        }
        .frame(width: width, height: height)
    }

    // MARK: - Curved Arc Timeline Page Indicator (from iOS)

    @ViewBuilder
    private func pageIndicator(span: CGFloat) -> some View {
        let count = featuredTitles.count
        let capsuleWidth: CGFloat = 56.0
        let arcRise: CGFloat = 12.0

        if count > 1 {
            TimelineView(.animation(minimumInterval: 1.0 / 30.0, paused: advanceStartedAt == nil)) { context in
                let progress = advanceStartedAt.map {
                    min(1.0, max(0.0, context.date.timeIntervalSince($0) / advanceInterval))
                } ?? 0.0

                ZStack {
                    ForEach(visibleSlots, id: \.self) { slot in
                        let distance = CGFloat(slot - position)
                        let emphasis = max(0.0, 1.0 - abs(distance))
                        let depth = min(abs(distance), 4.0)
                        let dotScale = max(0.40, 1.0 - depth * 0.16)
                        let arcX = CGFloat(sin(Double(distance) * 0.22) / sin(0.66)) * (span / 2.0 - 8.0)
                        let arcDepth = min(abs(distance) / 3.0, 1.5)

                        Capsule()
                            .fill(Color.white.opacity(0.18))
                            .overlay(alignment: .leading) {
                                GeometryReader { geometry in
                                    Rectangle()
                                        .fill(MelloTVTheme.accent)
                                        .frame(width: geometry.size.width * progress)
                                }
                                .opacity(emphasis)
                            }
                            .frame(width: 8.0 + (capsuleWidth - 8.0) * emphasis, height: 7.0)
                            .clipShape(Capsule())
                            .scaleEffect(dotScale)
                            .rotation3DEffect(
                                .degrees(reduceMotion ? 0 : Double(distance) * 4.0),
                                axis: (x: 0, y: 1, z: 0),
                                perspective: 0.35
                            )
                            .offset(x: arcX, y: 4.0 - arcDepth * arcDepth * arcRise)
                            .opacity(max(0.35, 1.0 - Double(depth) * 0.15))
                            .zIndex(-Double(abs(distance)))
                    }
                }
                .frame(width: span, height: 28)
            }
            .frame(height: 28)
            .allowsHitTesting(false)
        }
    }

    // MARK: - Auto-Advance Timer

    private func startTimer() {
        stopTimer()
        guard featuredTitles.count > 1, !reduceMotion else { return }
        advanceStartedAt = Date()
        timer = Timer.scheduledTimer(withTimeInterval: advanceInterval, repeats: true) { _ in
            Task { @MainActor in
                withAnimation(settleAnimation) {
                    position += 1
                }
                advanceStartedAt = Date()
            }
        }
    }

    private func stopTimer() {
        timer?.invalidate()
        timer = nil
        advanceStartedAt = nil
    }
}
