import SwiftUI

/// A 3D showroom stage hero carousel inspired by multi-car showroom platforms.
/// Four visible depth layers with gentle, centered perspective and graduated lighting.
struct FeaturedCarouselView: View {
    let titles: [MediaTitle]
    let viewportSize: CGSize
    var onSelect: (MediaTitle) -> Void
    var onPlay: (MediaTitle) -> Void

    @State private var position: Int = 0
    @State private var dragProgress: CGFloat = 0
    @State private var pausedProgress: Double = 0
    @State private var dragOffset: CGFloat = 0
    @State private var isDragging: Bool = false
    @State private var suppressTapUntil = Date.distantPast
    @State private var timer: Timer?
    @State private var advanceStartedAt: Date?
    private let advanceInterval: TimeInterval = 8
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Environment(\.accessibilityVoiceOverEnabled) private var voiceOverEnabled
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    private var currentIndex: Int { titleIndex(for: position) }

    private func titleIndex(for slot: Int) -> Int {
        let count = featuredTitles.count
        return count == 0 ? 0 : ((slot % count) + count) % count
    }

    // Stable occurrences keep moving in one direction through the wrap boundary.
    // Repeated titles use different occurrence IDs and recycle only offstage.
    private var visibleSlots: [Int] {
        featuredTitles.count > 1 ? Array((position - 5)...(position + 5)) : [position]
    }

    private var isLandscape: Bool { viewportSize.width > viewportSize.height }

    private var stageHeight: CGFloat {
        FeaturedCarouselLayout.stageHeight(viewport: viewportSize, accessibility: dynamicTypeSize.isAccessibilitySize)
    }

    private var settleAnimation: Animation {
        reduceMotion ? .easeOut(duration: 0.18) : .spring(response: 0.48, dampingFraction: 0.88)
    }

    private var featuredTitles: [MediaTitle] {
        Array(titles.prefix(7))
    }

    var body: some View {
        if !featuredTitles.isEmpty {
            GeometryReader { geo in
                let availableWidth = geo.size.width
                let layout = FeaturedCarouselLayout(
                    viewport: viewportSize,
                    width: availableWidth,
                    accessibility: dynamicTypeSize.isAccessibilitySize
                )
                let cardWidth = layout.cardWidth
                let cardHeight = layout.cardHeight
                let stepX = layout.stepX
                let swipeThreshold: CGFloat = cardWidth * 0.49

                // 3D Showroom Stage Container
                ZStack {
                    // The page owns light-mode atmosphere so no local gradient
                    // rectangle can end abruptly behind the indicator.
                    if colorScheme == .dark {
                        // Showroom floor spotlight aura centered beneath the stage
                        EllipticalGradient(
                            colors: [
                                MelloTVTheme.accent.opacity(colorScheme == .dark ? 0.09 : 0.04),
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

                    }

                    // Staged Cards (Center hero + flanking cards)
                    ForEach(visibleSlots, id: \.self) { slot in
                        stageCard(
                            title: featuredTitles[titleIndex(for: slot)],
                            index: slot,
                            cardWidth: cardWidth,
                            cardHeight: cardHeight,
                            stepX: stepX,
                            swipeThreshold: swipeThreshold
                        )
                    }
                }
                .frame(width: availableWidth, height: cardHeight)
                .padding(.top, isLandscape ? 6 : 54)
                .contentShape(Rectangle())
                // Observe the drag alongside poster buttons and vertical scrolling.
                // A recognized movement suppresses the button's release action.
                .simultaneousGesture(
                    DragGesture(minimumDistance: 10)
                        .onChanged { value in
                            suppressTapUntil = Date().addingTimeInterval(0.25)
                            guard featuredTitles.count > 1 else { return }
                            guard isDragging || abs(value.translation.width) > abs(value.translation.height) * 1.25 else { return }
                            if !isDragging {
                                isDragging = true
                                stopTimer()
                            }
                            dragOffset = max(-swipeThreshold, min(swipeThreshold, value.translation.width))
                            dragProgress = dragOffset / swipeThreshold
                        }
                        .onEnded { value in
                            suppressTapUntil = Date().addingTimeInterval(0.25)
                            guard isDragging else { return }
                            handleDragEnd(value: value, threshold: swipeThreshold)
                        }
                )
                .frame(height: stageHeight, alignment: .top)
                .overlay(alignment: .bottom) {
                    // Follow the outer edges of the second poster layer, with
                    // extra spread in the wider landscape composition.
                    pageIndicator(span: min(
                        availableWidth - 24,
                        cardWidth * (isLandscape ? 1.10 : 0.96) + stepX * 2
                    ))
                }
            }
            .frame(height: stageHeight)
            .onChange(of: scenePhase) { _, phase in
                if phase == .active { startTimer() } else { stopTimer() }
            }
            .onChange(of: reduceMotion) { _, _ in startTimer() }
            .onChange(of: voiceOverEnabled) { _, _ in startTimer() }
            .onChange(of: featuredTitles.map(\.id)) { _, _ in
                position = currentIndex
                dragOffset = 0
                dragProgress = 0
                isDragging = false
                startTimer()
            }
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

        let depth = min(absOffset, 3)
        let xOffset = effectiveOffset * stepX
        let scale = 1.0 - depth * 0.10
        let zIndex = 100.0 - Double(absOffset) * 20.0

        // 4°, 8°, and 12° give each layer a gentle, continuous turn.
        // Center anchoring avoids the sharply hinged look of the inner edge.
        let rotationY = Double(effectiveOffset) * 4.0

        // Compensate for the scaled poster's inset, then lift each rear layer
        // another 14 points. This makes the portrait stack rise on both sides.
        let yOffset = isLandscape ? depth * 2.0 : -depth * (cardHeight * 0.05 + 14.0)

        // Keep four layers opaque; fade only as cards exit stage beyond layer four
        let opacity = Double(max(0, 1 - max(0, absOffset - 3) * 2))

        let contentOpacity = max(0.0, 1.0 - Double(absOffset) * 1.8)
        let isCenter = absOffset < 0.35
        let proximity = Double(max(0, 1 - absOffset))
        let focus = proximity * proximity * (3 - 2 * proximity)

        Button {
            handleCardTap(title: title, effectiveOffset: effectiveOffset, count: count)
        } label: {
            ZStack {
                cardArtwork(title: title, width: cardWidth, height: cardHeight)
                    // Gentle depth of field: 0 / 0.8 / 1.6 / 2.4 points.
                    // Blur artwork only, keeping the rim and info control crisp.
                    .blur(radius: depth * 0.8, opaque: true)
                LinearGradient(
                    colors: [
                        Color.black.opacity(Double(depth) * 0.07),
                        Color.black.opacity(Double(depth) * 0.12)
                    ],
                    startPoint: .top,
                    endPoint: .bottom
                )
            }
            .frame(width: cardWidth, height: cardHeight)
            .contentShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
        }
        .buttonStyle(HeroPosterButtonStyle())
        .accessibilityLabel(isCenter ? "Play \(title.title)" : "Show \(title.title)")
        .allowsHitTesting(absOffset <= 3)
        .overlay(alignment: .topTrailing) {
            BrandTVMark()
                .padding(12)
                .opacity(contentOpacity)
        }
        .overlay(alignment: .bottomTrailing) {
            Button {
                guard !isDragging, Date() >= suppressTapUntil else { return }
                onSelect(title)
            } label: {
                CardDetailsGlyph()
            }
            .buttonStyle(CardScaleButtonStyle(scale: 0.94))
            .accessibilityLabel("Details for \(title.title)")
            .padding(.trailing, 12)
            .padding(.bottom, 2)
            .opacity(contentOpacity)
            .allowsHitTesting(isCenter)
            .accessibilityHidden(!isCenter)
        }
        .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 22, style: .continuous)
                .strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 0.75)
                .allowsHitTesting(false)
        }
        .background {
            posterLight(title: title, width: cardWidth, height: cardHeight, diffuse: false)
                .opacity(focus)
                .allowsHitTesting(false)
                .accessibilityHidden(true)
        }
        .shadow(
            color: Color.black.opacity(
                colorScheme == .dark ? 0.20 + focus * 0.55 : 0.10 + focus * 0.22
            ),
            radius: 5 + 19 * focus,
            x: 0,
            y: 5 + 11 * focus
        )
        .background {
            posterLight(title: title, width: cardWidth, height: cardHeight, diffuse: true)
                .opacity(focus * (colorScheme == .dark ? 0.85 : 0.55))
                .allowsHitTesting(false)
                .accessibilityHidden(true)
        }
        // Gentle perspective preserves readable artwork on every depth layer.
        .rotation3DEffect(
            .degrees(reduceMotion ? 0 : rotationY),
            axis: (x: 0, y: 1, z: 0),
            anchor: .center,
            perspective: 0.35
        )
        .scaleEffect(scale)
        .offset(x: xOffset, y: yOffset)
        .zIndex(zIndex)
        .opacity(opacity)
        .accessibilityElement(children: isCenter ? .contain : .ignore)
        .accessibilityLabel(isCenter ? "" : "Show \(title.title)")
        .accessibilityHidden(!isCenter)
    }

    private func posterLight(title: MediaTitle, width: CGFloat, height: CGFloat, diffuse: Bool) -> some View {
        // The poster itself supplies local edge colors instead of a fixed accent.
        // Blur blends them into a soft frame and a wider wash behind the artwork.
        cardArtwork(title: title, width: width + 12, height: height + 12)
            .saturation(1.35)
            .brightness(0.12)
            .blur(radius: 10, opaque: true)
            .overlay {
                LinearGradient(
                    colors: [.black.opacity(0.78), .black.opacity(0.25), .clear],
                    startPoint: .top,
                    endPoint: .bottom
                )
            }
            .mask {
                RoundedRectangle(cornerRadius: 28, style: .continuous)
                    .strokeBorder(lineWidth: diffuse ? 14 : 6)
            }
            .blur(radius: diffuse ? 18 : 1.5)
            .frame(width: width, height: height)
    }

    // MARK: - Page Indicator

    @ViewBuilder
    private func pageIndicator(span: CGFloat) -> some View {
        let count = featuredTitles.count
        let capsuleWidth: CGFloat = isLandscape ? 56.0 : 48.0
        let arcRise: CGFloat = isLandscape ? 13.0 : 10.0

        if count > 1 {
            TimelineView(.animation(minimumInterval: 1.0 / 30.0, paused: advanceStartedAt == nil)) { context in
                let progress = advanceStartedAt.map {
                    min(1.0, max(0.0, context.date.timeIntervalSince($0) / advanceInterval))
                } ?? pausedProgress

                ZStack {
                    ForEach(visibleSlots, id: \.self) { slot in
                        let distance = CGFloat(slot - position) + dragProgress
                        let emphasis = max(0.0, 1.0 - abs(distance))
                        let depth = min(abs(distance), 4.0)
                        // A shallow arc: smaller, closer-spaced dots recede behind
                        // the active capsule using the same gentle turn as the cards.
                        let dotScale = max(0.40, 1.0 - depth * 0.16)
                        // Map the outer visible dots to the second-layer footprint.
                        // Sinusoidal spacing gives the arc a mild perspective taper.
                        let arcX = CGFloat(sin(Double(distance) * 0.22) / sin(0.66)) * (span / 2.0 - 8.0)
                        let arcDepth = min(abs(distance) / 3.0, 1.5)

                        Capsule()
                            .fill(MelloTVTheme.text.opacity(0.16))
                            .overlay(alignment: .leading) {
                                GeometryReader { geometry in
                                    Rectangle()
                                        .fill(MelloTVTheme.accent)
                                        .frame(width: geometry.size.width * progress)
                                }
                                .opacity(emphasis)
                                .transaction { $0.animation = nil }
                            }
                            .frame(width: 8.0 + (capsuleWidth - 8.0) * emphasis, height: 8.0)
                            .clipShape(Capsule())
                            .scaleEffect(dotScale)
                            .rotation3DEffect(
                                .degrees(reduceMotion ? 0 : Double(distance) * 4.0),
                                axis: (x: 0, y: 1, z: 0),
                                perspective: 0.35
                            )
                            .offset(x: arcX, y: 5.0 - arcDepth * arcDepth * arcRise)
                            .opacity(max(0.35, 1.0 - Double(depth) * 0.15))
                            .zIndex(-Double(abs(distance)))
                            .accessibilityHidden(true)
                    }
                }
                .frame(width: max(capsuleWidth + 16.0, span), height: 32)
                .mask {
                    LinearGradient(
                        stops: [
                            .init(color: .clear, location: 0.0),
                            .init(color: .black, location: 0.04),
                            .init(color: .black, location: 0.96),
                            .init(color: .clear, location: 1.0)
                        ],
                        startPoint: .leading,
                        endPoint: .trailing
                    )
                }
            }
            .frame(height: 32)
            .frame(maxWidth: .infinity)
            .contentShape(Rectangle())
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Featured titles")
            .accessibilityValue("\(currentIndex + 1) of \(featuredTitles.count)")
            .accessibilityAdjustableAction { direction in
                guard featuredTitles.count > 1 else { return }
                let step = direction == .increment ? 1 : -1
                withAnimation(settleAnimation) {
                    position += step
                }
                startTimer()
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
                        artworkPlaceholder(title: title, width: width, height: height)
                    case .empty:
                        ZStack {
                            artworkPlaceholder(title: title, width: width, height: height)
                            ProgressView()
                                .tint(.white.opacity(0.6))
                        }
                    @unknown default:
                        artworkPlaceholder(title: title, width: width, height: height)
                    }
                }
            } else {
                artworkPlaceholder(title: title, width: width, height: height)
            }
        }
        .frame(width: width, height: height)
    }

    private func artworkPlaceholder(title: MediaTitle, width: CGFloat, height: CGFloat) -> some View {
        ZStack {
            MelloTVTheme.cardBackground
            VStack(spacing: 8) {
                PhosphorIcon("film-slate", size: 36)
                    .foregroundStyle(MelloTVTheme.muted.opacity(0.5))
                Text(title.title)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(MelloTVTheme.muted)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 8)
                    .lineLimit(2)
            }
        }
        .frame(width: width, height: height)
    }

    // MARK: - Gesture & Offset Math

    private func computeEffectiveOffset(index: Int, count: Int, threshold: CGFloat) -> CGFloat {
        guard count > 0 else { return 0 }
        let baseOffset = CGFloat(index - position)
        return baseOffset + dragProgress
    }

    private func handleDragEnd(value: DragGesture.Value, threshold: CGFloat) {
        let dragDistance = value.translation.width
        let velocity = value.velocity.width
        let triggerDistance = threshold * 0.35

        var stepChange = 0
        if dragDistance < -triggerDistance || velocity < -250 {
            stepChange = 1
        } else if dragDistance > triggerDistance || velocity > 250 {
            stepChange = -1
        }

        if stepChange != 0 {
            UISelectionFeedbackGenerator().selectionChanged()
        }

        withAnimation(settleAnimation) {
            position += stepChange
            dragOffset = 0
            dragProgress = 0
            isDragging = false
        }
        startTimer()
    }

    private func handleCardTap(title: MediaTitle, effectiveOffset: CGFloat, count: Int) {
        guard !isDragging, Date() >= suppressTapUntil else { return }

        if abs(effectiveOffset) < 0.35 {
            onPlay(title)
        } else if effectiveOffset < -0.35 {
            UISelectionFeedbackGenerator().selectionChanged()
            withAnimation(settleAnimation) {
                position -= 1
            }
            startTimer()
        } else if effectiveOffset > 0.35 {
            UISelectionFeedbackGenerator().selectionChanged()
            withAnimation(settleAnimation) {
                position += 1
            }
            startTimer()
        }
    }

    // MARK: - Timer

    private func startTimer() {
        stopTimer()
        guard featuredTitles.count > 1, !reduceMotion, !voiceOverEnabled else { return }
        pausedProgress = 0
        advanceStartedAt = Date()
        timer = Timer.scheduledTimer(withTimeInterval: advanceInterval, repeats: true) { _ in
            Task { @MainActor in
                guard !isDragging else { return }
                withAnimation(settleAnimation) {
                    position += 1
                }
                advanceStartedAt = Date()
            }
        }
    }

    private func stopTimer() {
        if let start = advanceStartedAt {
            pausedProgress = min(1.0, max(0.0, Date().timeIntervalSince(start) / advanceInterval))
        }
        timer?.invalidate()
        timer = nil
        advanceStartedAt = nil
    }
}

// Shared sizing keeps loading and loaded artwork in exactly the same positions.
private struct FeaturedCarouselLayout {
    let viewport: CGSize
    let width: CGFloat
    let accessibility: Bool

    var isLandscape: Bool { viewport.width > viewport.height }
    static func stageHeight(viewport: CGSize, accessibility: Bool) -> CGFloat {
        viewport.width > viewport.height
            ? max(100, min(440, viewport.height - 24))
            : (accessibility ? 594 : 494)
    }
    var height: CGFloat { Self.stageHeight(viewport: viewport, accessibility: accessibility) }
    var cardWidth: CGFloat {
        isLandscape ? min(286, width * 0.32, (height - 42) / 1.5) : min(width * 0.70, 286)
    }
    var cardHeight: CGFloat {
        isLandscape ? cardWidth * 1.5 : min(height - 94, cardWidth * 1.46 + (accessibility ? 100 : 0))
    }
    var stepX: CGFloat { min(cardWidth * 0.64, max(1, (width - cardWidth * 0.70 - 16) / 6)) }
}

struct FeaturedCarouselSkeleton: View {
    let viewportSize: CGSize
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        GeometryReader { geometry in
            let layout = FeaturedCarouselLayout(
                viewport: viewportSize,
                width: geometry.size.width,
                accessibility: dynamicTypeSize.isAccessibilitySize
            )
            TimelineView(.animation(minimumInterval: 1.0 / 30, paused: reduceMotion)) { context in
                let pulse = reduceMotion ? 0.5 : (sin(context.date.timeIntervalSinceReferenceDate * .pi / 1.2) + 1) / 2
                ZStack {
                    ForEach(-3...3, id: \.self) { layer in
                        let depth = CGFloat(abs(layer))
                        RoundedRectangle(cornerRadius: 22, style: .continuous)
                            .fill(MelloTVTheme.secondaryBackground)
                            .overlay {
                                RoundedRectangle(cornerRadius: 22, style: .continuous)
                                    .fill(MelloTVTheme.text.opacity(0.025 + pulse * 0.035))
                            }
                            .overlay {
                                RoundedRectangle(cornerRadius: 22, style: .continuous)
                                    .strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 0.75)
                            }
                            .frame(width: layout.cardWidth, height: layout.cardHeight)
                            .rotation3DEffect(
                                .degrees(reduceMotion ? 0 : Double(layer) * 4),
                                axis: (x: 0, y: 1, z: 0),
                                perspective: 0.35
                            )
                            .scaleEffect(1 - depth * 0.10)
                            .offset(
                                x: CGFloat(layer) * layout.stepX,
                                y: layout.isLandscape ? depth * 2 : -depth * (layout.cardHeight * 0.05 + 14)
                            )
                            .opacity(1 - Double(depth) * 0.15)
                            .zIndex(-Double(depth))
                    }
                }
                .frame(width: geometry.size.width, height: layout.cardHeight)
                .padding(.top, layout.isLandscape ? 6 : 54)
            }
        }
        .frame(height: FeaturedCarouselLayout.stageHeight(
            viewport: viewportSize, accessibility: dynamicTypeSize.isAccessibilitySize
        ))
        .allowsHitTesting(false)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Loading featured titles")
    }
}

/// Artwork is a solid stage surface, including while a touch is held.
private struct HeroPosterButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
    }
}
