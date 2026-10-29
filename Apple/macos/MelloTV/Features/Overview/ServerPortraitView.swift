import SwiftUI

/// A decorative ceramic enclosure. The small front indicator reflects server
/// state; the sculpture does not represent measured traffic or hardware.
struct ServerPortraitView: View {
    let isOnline: Bool
    let needsAttention: Bool

    var body: some View {
        GeometryReader { geometry in
            let side = min(330, geometry.size.width * 0.84, geometry.size.height * 1.13)
            ZStack {
                Ellipse()
                    .fill(Color(red: 0.025, green: 0.16, blue: 0.12).opacity(0.38))
                    .frame(width: side * 0.88, height: side * 0.16)
                    .blur(radius: 18)
                    .offset(x: 14, y: side * 0.35)

                enclosure(side: side)
                    .rotation3DEffect(.degrees(27), axis: (x: 1, y: 0, z: 0), perspective: 0.25)
                    .rotation3DEffect(.degrees(-13), axis: (x: 0, y: 1, z: 0), perspective: 0.25)
                    .rotationEffect(.degrees(-12))
                    .shadow(color: Color(red: 0.025, green: 0.12, blue: 0.09).opacity(0.28), radius: 20, x: 8, y: 24)
                    .offset(y: -12)
            }
            .frame(width: geometry.size.width, height: geometry.size.height)
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    private func enclosure(side: CGFloat) -> some View {
        ZStack {
            // Closely spaced silhouettes form the rounded ceramic sidewall.
            ForEach(0..<16, id: \.self) { layer in
                RoundedRectangle(cornerRadius: side * 0.22, style: .continuous)
                    .fill(LinearGradient(
                        colors: [Color(red: 0.77, green: 0.77, blue: 0.69), Color(red: 0.52, green: 0.57, blue: 0.49)],
                        startPoint: .topLeading, endPoint: .bottomTrailing
                    ))
                    .offset(y: CGFloat(15 - layer) * 1.6)
            }

            RoundedRectangle(cornerRadius: side * 0.22, style: .continuous)
                .fill(LinearGradient(stops: [
                    .init(color: Color(red: 0.99, green: 0.98, blue: 0.90), location: 0),
                    .init(color: Color(red: 0.90, green: 0.90, blue: 0.81), location: 0.48),
                    .init(color: Color(red: 0.74, green: 0.78, blue: 0.68), location: 1)
                ], startPoint: .topLeading, endPoint: .bottomTrailing))
                .overlay {
                    RoundedRectangle(cornerRadius: side * 0.22, style: .continuous)
                        .strokeBorder(LinearGradient(colors: [.white.opacity(0.85), .white.opacity(0.05)],
                                                     startPoint: .topLeading, endPoint: .bottomTrailing), lineWidth: 1.5)
                }

            VStack(spacing: side * 0.055) {
                ZStack {
                    RoundedRectangle(cornerRadius: side * 0.09, style: .continuous)
                        .fill(Color(red: 0.65, green: 0.69, blue: 0.60))
                        .shadow(color: .white.opacity(0.65), radius: 1, x: 0, y: 2)
                    RoundedRectangle(cornerRadius: side * 0.075, style: .continuous)
                        .fill(LinearGradient(colors: [Color(red: 0.06, green: 0.15, blue: 0.13),
                                                       Color(red: 0.15, green: 0.25, blue: 0.21)],
                                             startPoint: .topLeading, endPoint: .bottomTrailing))
                        .padding(5)
                    Image(systemName: "play.fill")
                        .font(.system(size: side * 0.12, weight: .semibold))
                        .foregroundStyle(Color(red: 0.92, green: 0.94, blue: 0.84))
                        .offset(x: side * 0.009)
                        .shadow(color: .black.opacity(0.18), radius: 2, y: 2)
                }
                .frame(width: side * 0.39, height: side * 0.29)

                Text("Mello TV")
                    .font(.system(size: side * 0.036, weight: .semibold, design: .rounded))
                    .tracking(1.1)
                    .foregroundStyle(Color(red: 0.32, green: 0.39, blue: 0.32))
            }
            .offset(y: -side * 0.02)

            HStack(spacing: 4) {
                ForEach(0..<7, id: \.self) { _ in
                    Capsule()
                        .fill(Color(red: 0.35, green: 0.43, blue: 0.35).opacity(0.65))
                        .frame(width: 2, height: 5)
                }
                Spacer()
                Capsule()
                    .fill(indicatorColor)
                    .frame(width: side * 0.065, height: 4)
                    .shadow(color: indicatorColor.opacity(isOnline ? 0.45 : 0), radius: 4)
            }
            .padding(.horizontal, side * 0.20)
            .offset(y: side * 0.35 + 9)
        }
        .frame(width: side, height: side * 0.71)
    }

    private var indicatorColor: Color {
        if needsAttention { return .orange }
        return isOnline ? Color(red: 0.54, green: 0.91, blue: 0.64) : Color(red: 0.37, green: 0.43, blue: 0.36)
    }
}
