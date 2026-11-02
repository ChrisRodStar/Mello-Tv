import SwiftUI

/// The complete licensed posters retain their titles, credits, and original proportions.
struct OnboardingLibraryArtwork: View {
    var body: some View {
        GeometryReader { geometry in
            let posterHeight = min(geometry.size.height * 0.92, geometry.size.width / 1.5)
            let posterWidth = posterHeight * 0.707
            ZStack {
                poster(.bigBuckBunny, height: posterHeight)
                    .rotationEffect(.degrees(-13))
                    .offset(x: -posterWidth * 0.45, y: 5)
                    .scaleEffect(0.88)
                poster(.elephantsDream, height: posterHeight)
                    .rotationEffect(.degrees(12))
                    .offset(x: posterWidth * 0.45, y: 5)
                    .scaleEffect(0.88)
                poster(.sintel, height: posterHeight)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .accessibilityHidden(true)
    }

    private func poster(_ film: OnboardingPoster, height: CGFloat) -> some View {
        Image("onboarding-" + film.rawValue)
            .resizable()
            .scaledToFit()
            .frame(width: height * 0.707, height: height)
            .shadow(color: .black.opacity(0.22), radius: 18, x: 0, y: 14)
    }
}
