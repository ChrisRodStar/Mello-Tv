import SwiftUI

/// A 3D showroom stage hero carousel inspired by multi-car showroom platforms.
/// Features an active hero card front-and-center flanked by side cards angled inward
/// toward the center spotlight using hinged inner-edge perspective anchors.
struct FeaturedCarouselView: View {
    let titles: [MediaTitle]
    var onSelect: (MediaTitle) -> Void
    var onPlay: (MediaTitle) -> Void

    @State private var currentIndex: Int = 0
    @State private var dragOffset: CGFloat = 0
    @State private var isDragging: Bool = false
    @State private var timer: Timer?
    @Environment(\.colorScheme) private var colorScheme

    private var featuredTitles: [MediaTitle] {
        Array(titles.prefix(7))
    }

    var body: some View {
        if !featuredTitles.isEmpty {
            GeometryReader { geo in
                let availableWidth = geo.size.width
                let cardWidth: CGFloat = min(availableWidth * 0.70, 275)
                let cardHeight: CGFloat = cardWidth * 1.45
                let stepX: CGFloat = cardWidth * 0.44
                let swipeThreshold: CGFloat = cardWidth * 0.65

                // 3D Showroom Stage Container
                ZStack {
                    // Showroom floor spotlight aura centered beneath the stage
                    EllipticalGradient(
                        colors: [
                            FamilyStreamTheme.accent.opacity(0.12),
                            Color.black.opacity(0.05),
                            Color.clear
                        ],
                        center: .center,
                        startRadiusFraction: 0.05,
                        endRadiusFraction: 0.70
                    )
                    .frame(width: availableWidth, height: cardHeight * 0.95)
                    .offset(y: 16)
                    .allowsHitTesting(false)

                    // Staged Cards (Center hero + perspective-anchored flanking cards)
                    ForEach(Array(featuredTitles.enumerated()), id: \.element.id) { index, title in
                        stageCard(
                            title: title,
                            index: index,
                            cardWidth: cardWidth,
                            cardHeight: cardHeight,
                            stepX: stepX,
                            swipeThreshold: swipeThreshold
                        )
                    }
                }
                .frame(width: availableWidth, height: cardHeight)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture()
                        .onChanged { value in
                            if !isDragging {
                                isDragging = true
                                stopTimer()
                            }
                            dragOffset = value.translation.width
                        }
                        .onEnded { value in
                            handleDragEnd(value: value, threshold: swipeThreshold)
                        }
                )
            }
            .frame(height: 410)
            .onAppear {
                startTimer()
            }
            .onDisappear {
                stopTimer()
            }
        }
    }

    // MARK: - 3D Showroom Card

    @ViewBuilder
    private func stageCard(
        title: MediaTitle,
        index: Int,
        cardWidth: CGFloat,
        cardHeight: CGFloat,
        stepX: CGFloat,
        swipeThreshold: CGFloat
    ) -> some View {
        let count = featuredTitles.count
        let effectiveOffset = computeEffectiveOffset(index: index, count: count, threshold: swipeThreshold)
        let absOffset = abs(effectiveOffset)

        if absOffset <= 2.2 {
            let xOffset = effectiveOffset * stepX
            let scale = max(0.68, 1.0 - absOffset * 0.16)
            let zIndex = 100.0 - Double(absOffset) * 25.0

            // Showroom Perspective Angles:
            // Flanking cards are hinged at their inner edge (tucked behind the center card)
            // and angled inward by 15° toward the center spotlight.
            let rotationY: Double = {
                if absOffset < 0.01 { return 0 }
                let sign: Double = effectiveOffset > 0 ? 1.0 : -1.0
                return sign * 15.0 * min(1.0, absOffset)
            }()

            let rotationAnchor: UnitPoint = {
                if effectiveOffset > 0.05 { return .leading }
                if effectiveOffset < -0.05 { return .trailing }
                return .center
            }()

            // Slight horizon elevation for depth staging
            let yOffset = -absOffset * 6.0

            let opacity: Double = {
                if absOffset <= 1.0 { return 1.0 }
                if absOffset <= 2.0 { return max(0.0, 1.0 - (absOffset - 1.0) * 0.9) }
                return 0.0
            }()

            let contentOpacity = max(0.0, 1.0 - Double(absOffset) * 2.8)
            let isCenter = absOffset < 0.35

            ZStack(alignment: .bottom) {
                // High-resolution Artwork
                cardArtwork(title: title, width: cardWidth, height: cardHeight)

                // Multi-Stage Apple Scrim Gradient for text readability
                LinearGradient(
                    stops: [
                        .init(color: .clear, location: 0.28),
                        .init(color: Color.black.opacity(0.32), location: 0.50),
                        .init(color: Color.black.opacity(0.80), location: 0.72),
                        .init(color: Color.black.opacity(0.96), location: 1.0)
                    ],
                    startPoint: .top,
                    endPoint: .bottom
                )
                .frame(width: cardWidth, height: cardHeight)

                // Ambient dimming overlay for flanking background cards
                Color.black.opacity(min(0.55, Double(absOffset) * 0.50))
                    .frame(width: cardWidth, height: cardHeight)

                // Bottom Content (Category kicker, Title, Subtitle, Apple Action Pills)
                VStack(spacing: 8) {
                    // Category / Genre Kicker
                    if let genre = title.genres.first, !genre.isEmpty {
                        Text(genre.uppercased())
                            .font(.system(size: 10, weight: .bold))
                            .foregroundStyle(Color.white.opacity(0.70))
                            .tracking(1.0)
                    }

                    // Title
                    Text(title.title)
                        .font(.system(size: 20, weight: .bold, design: .rounded))
                        .foregroundStyle(.white)
                        .multilineTextAlignment(.center)
                        .lineLimit(2)
                        .shadow(color: .black.opacity(0.95), radius: 6, x: 0, y: 2)
                        .padding(.horizontal, 10)

                    // Minimal Editorial Subtitle Line
                    let subtitle = cardSubtitle(for: title)
                    if !subtitle.isEmpty {
                        Text(subtitle)
                            .font(.system(size: 11, weight: .medium))
                            .foregroundStyle(Color.white.opacity(0.75))
                            .lineLimit(1)
                            .shadow(color: .black.opacity(0.8), radius: 3, x: 0, y: 1)
                    }

                    // Apple-Style Action Capsules (Play & Details)
                    HStack(spacing: 10) {
                        Button {
                            onPlay(title)
                        } label: {
                            HStack(spacing: 6) {
                                PhosphorIcon("play", size: 14, weight: .fill)
                                Text("Play")
                                    .font(.subheadline.weight(.bold))
                            }
                            .foregroundStyle(Color(red: 18/255, green: 18/255, blue: 20/255))
                            .frame(maxWidth: .infinity)
                            .frame(height: 38)
                            .background(Color.white, in: Capsule())
                            .shadow(color: .black.opacity(0.35), radius: 4, y: 2)
                        }
                        .buttonStyle(CardScaleButtonStyle(scale: 0.96))

                        Button {
                            onSelect(title)
                        } label: {
                            HStack(spacing: 6) {
                                PhosphorIcon("info", size: 16, weight: .bold)
                                Text("Details")
                                    .font(.subheadline.weight(.semibold))
                            }
                            .foregroundStyle(.white)
                            .frame(maxWidth: .infinity)
                            .frame(height: 38)
                            .background(Color.white.opacity(0.20), in: Capsule())
                            .overlay(
                                Capsule()
                                    .stroke(Color.white.opacity(0.25), lineWidth: 0.5)
                            )
                        }
                        .buttonStyle(CardScaleButtonStyle(scale: 0.96))
                    }
                    .padding(.horizontal, 14)
                    .padding(.top, 4)
                }
                .padding(.bottom, 16)
                .opacity(contentOpacity)
            }
            .frame(width: cardWidth, height: cardHeight)
            .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .stroke(
                        isCenter
                            ? LinearGradient(
                                stops: [
                                    .init(color: Color.white.opacity(0.35), location: 0.0),
                                    .init(color: Color.white.opacity(0.08), location: 0.5),
                                    .init(color: Color.black.opacity(0.40), location: 1.0)
                                ],
                                startPoint: .topLeading,
                                endPoint: .bottomTrailing
                            )
                            : LinearGradient(
                                colors: [FamilyStreamTheme.subtleBorder, FamilyStreamTheme.subtleBorder],
                                startPoint: .top,
                                endPoint: .bottom
                            ),
                        lineWidth: 0.75
                    )
            )
            // Center card drop shadows
            .shadow(
                color: (isCenter && colorScheme == .dark) ? Color.white.opacity(0.12) : Color.clear,
                radius: 8,
                x: -4,
                y: -4
            )
            .shadow(
                color: isCenter
                    ? Color.black.opacity(colorScheme == .dark ? 0.70 : 0.28)
                    : Color.black.opacity(colorScheme == .dark ? 0.35 : 0.16),
                radius: isCenter ? 22 : 10,
                x: 0,
                y: isCenter ? 12 : 5
            )
            // 3D Showroom Transforms: Inward-hinged rotation + subtle easel lean
            .rotation3DEffect(
                .degrees(rotationY),
                axis: (x: 0, y: 1, z: 0),
                anchor: rotationAnchor,
                perspective: 0.65
            )
            .rotation3DEffect(
                .degrees(3.0),
                axis: (x: 1, y: 0, z: 0),
                perspective: 0.85
            )
            .scaleEffect(scale)
            .offset(x: xOffset, y: yOffset)
            .zIndex(zIndex)
            .opacity(opacity)
            .onTapGesture {
                handleCardTap(title: title, effectiveOffset: effectiveOffset, count: count)
            }
        }
    }

    // MARK: - Card Artwork

    private func cardArtwork(title: MediaTitle, width: CGFloat, height: CGFloat) -> some View {
        Group {
            if let imageURL = title.posterURL ?? title.backdropURL {
                AsyncImage(url: imageURL) { phase in
                    switch phase {
                    case .success(let img):
                        img
                            .resizable()
                            .scaledToFill()
                            .frame(width: width, height: height)
                            .clipped()
                    case .failure:
                        artworkPlaceholder(width: width, height: height)
                    case .empty:
                        ZStack {
                            artworkPlaceholder(width: width, height: height)
                            ProgressView()
                                .tint(.white.opacity(0.6))
                        }
                    @unknown default:
                        artworkPlaceholder(width: width, height: height)
                    }
                }
            } else {
                artworkPlaceholder(width: width, height: height)
            }
        }
        .frame(width: width, height: height)
    }

    private func artworkPlaceholder(width: CGFloat, height: CGFloat) -> some View {
        ZStack {
            FamilyStreamTheme.cardBackground
            PhosphorIcon("film-slate", size: 36)
                .foregroundStyle(FamilyStreamTheme.muted.opacity(0.5))
        }
        .frame(width: width, height: height)
    }

    // MARK: - Metadata Subtitle

    private func cardSubtitle(for title: MediaTitle) -> String {
        var segments: [String] = []
        if !title.year.isEmpty {
            segments.append(title.year)
        }
        segments.append(title.type == .movie ? "Movie" : "Series")
        if title.rating > 0 {
            segments.append(String(format: "%.1f", title.rating))
        }
        return segments.joined(separator: "  •  ")
    }

    // MARK: - Gesture & Offset Math

    private func computeEffectiveOffset(index: Int, count: Int, threshold: CGFloat) -> CGFloat {
        guard count > 0 else { return 0 }

        var diff = (index - currentIndex) % count
        if diff > count / 2 {
            diff -= count
        } else if diff < -count / 2 {
            diff += count
        }

        let dragProgress = dragOffset / threshold
        return CGFloat(diff) + dragProgress
    }

    private func handleDragEnd(value: DragGesture.Value, threshold: CGFloat) {
        let dragDistance = value.translation.width
        let velocity = value.velocity.width
        let triggerDistance = threshold * 0.45

        var stepChange = 0
        if dragDistance < -triggerDistance || velocity < -300 {
            stepChange = 1
        } else if dragDistance > triggerDistance || velocity > 300 {
            stepChange = -1
        }

        if stepChange != 0 {
            UIImpactFeedbackGenerator(style: .medium).impactOccurred()
        }

        withAnimation(.spring(response: 0.38, dampingFraction: 0.82)) {
            let count = featuredTitles.count
            if count > 0 && stepChange != 0 {
                currentIndex = (currentIndex + stepChange + count) % count
            }
            dragOffset = 0
            isDragging = false
        }
        startTimer()
    }

    private func handleCardTap(title: MediaTitle, effectiveOffset: CGFloat, count: Int) {
        if abs(effectiveOffset) < 0.35 {
            // Center card tapped -> open title details
            onSelect(title)
        } else if effectiveOffset < -0.35 {
            // Left card tapped -> navigate to left card
            UIImpactFeedbackGenerator(style: .light).impactOccurred()
            withAnimation(.spring(response: 0.38, dampingFraction: 0.82)) {
                currentIndex = (currentIndex - 1 + count) % count
            }
            startTimer()
        } else if effectiveOffset > 0.35 {
            // Right card tapped -> navigate to right card
            UIImpactFeedbackGenerator(style: .light).impactOccurred()
            withAnimation(.spring(response: 0.38, dampingFraction: 0.82)) {
                currentIndex = (currentIndex + 1) % count
            }
            startTimer()
        }
    }

    // MARK: - Timer

    private func startTimer() {
        stopTimer()
        guard featuredTitles.count > 1 else { return }
        timer = Timer.scheduledTimer(withTimeInterval: 6.5, repeats: true) { _ in
            Task { @MainActor in
                guard !isDragging else { return }
                withAnimation(.spring(response: 0.42, dampingFraction: 0.82)) {
                    currentIndex = (currentIndex + 1) % featuredTitles.count
                }
            }
        }
    }

    private func stopTimer() {
        timer?.invalidate()
        timer = nil
    }
}
