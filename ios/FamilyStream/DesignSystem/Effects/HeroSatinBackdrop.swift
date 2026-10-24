import SwiftUI
import CoreMotion
import Observation

/// Only this background reads motion; catalog content never redraws for tilt updates.
struct HeroSatinBackdrop: View {
    var strength: Float = 1
    var reactsToMotion = true
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var motion = HeroSurfaceMotion.shared
    @State private var owner = UUID()
    @State private var isVisible = false

    var body: some View {
        GeometryReader { geometry in
            let landscape = geometry.size.width > geometry.size.height
            let delta = reduceMotion || !reactsToMotion ? SIMD3<Float>.zero : motion.gravity - motion.reference
            let direction: Float = reactsToMotion && motion.gravity.x < 0 ? -1 : 1
            let tilt = landscape
                ? SIMD2<Float>(delta.y * direction, delta.x * direction)
                : SIMD2<Float>(delta.x, delta.y)

            Rectangle()
                .fill(FamilyStreamTheme.background)
                .colorEffect(ShaderLibrary.satinSurface(
                    .float2(Float(geometry.size.width), Float(geometry.size.height)),
                    .float2(tilt.x, tilt.y),
                    .float(colorScheme == .dark ? 1 : 0),
                    .float(strength)
                ))
                .onChange(of: landscape) { _, _ in
                    if reactsToMotion { motion.recenter() }
                }
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
        .onAppear { isVisible = true; updateMotion() }
        .onDisappear { isVisible = false; updateMotion() }
        .onChange(of: scenePhase) { _, _ in updateMotion() }
        .onChange(of: reduceMotion) { _, _ in updateMotion() }
    }

    private func updateMotion() {
        motion.setActive(reactsToMotion && isVisible && scenePhase == .active && !reduceMotion, owner: owner)
    }
}

/// One sensor manager shared by the three browsing tabs, with explicit ownership.
@MainActor
@Observable
private final class HeroSurfaceMotion {
    static let shared = HeroSurfaceMotion()
    private(set) var gravity = SIMD3<Float>(0, -1, 0)
    private(set) var reference = SIMD3<Float>(0, -1, 0)
    @ObservationIgnored private let manager = CMMotionManager()
    @ObservationIgnored private var owners: Set<UUID> = []
    @ObservationIgnored private var needsReference = true

    func recenter() { needsReference = true }

    func setActive(_ active: Bool, owner: UUID) {
        if active { owners.insert(owner) } else { owners.remove(owner) }
        guard !owners.isEmpty, manager.isDeviceMotionAvailable else {
            manager.stopDeviceMotionUpdates()
            needsReference = true
            return
        }
        guard !manager.isDeviceMotionActive else { return }
        manager.deviceMotionUpdateInterval = ProcessInfo.processInfo.isLowPowerModeEnabled ? 1.0 / 15 : 1.0 / 30
        manager.startDeviceMotionUpdates(to: .main) { [weak self] data, _ in
            guard let data else { return }
            let sample = SIMD3<Float>(Float(data.gravity.x), Float(data.gravity.y), Float(data.gravity.z))
            Task { @MainActor [weak self] in
                guard let self, !self.owners.isEmpty else { return }
                if self.needsReference {
                    self.gravity = sample
                    self.reference = sample
                    self.needsReference = false
                } else {
                    let filtered = self.gravity + (sample - self.gravity) * 0.16
                    let difference = filtered - self.gravity
                    if abs(difference.x) + abs(difference.y) + abs(difference.z) > 0.0008 {
                        self.gravity = filtered
                    }
                }
            }
        }
    }
}

/// One full-page coordinate space avoids seams at status bars and sheet chrome.
struct CatalogPageSurface: View {
    var heroOffset: CGFloat = 0

    var body: some View {
        GeometryReader { geometry in
            ZStack(alignment: .top) {
                HeroSatinBackdrop(strength: 0.20, reactsToMotion: false)
                HeroSatinBackdrop()
                    .frame(height: geometry.size.width > geometry.size.height ? geometry.size.height : 620)
                    .mask {
                        LinearGradient(stops: [
                            .init(color: .black, location: 0),
                            .init(color: .black, location: 0.55),
                            .init(color: .black.opacity(0.40), location: 0.80),
                            .init(color: .clear, location: 1)
                        ], startPoint: .top, endPoint: .bottom)
                    }
                    .offset(y: min(0, heroOffset))
            }
            .clipped()
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}
