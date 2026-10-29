import AppKit
import SwiftUI
import MelloKit

/// An ornamental portrait of the local server. Only the indicator reflects state;
/// the enclosure and etched signal lines do not imply traffic or measured load.
struct ServerPortraitView: View {
    let isOnline: Bool
    let needsAttention: Bool

    var body: some View {
        GeometryReader { geometry in
            let width = min(210, geometry.size.width - 16)
            let enclosureHeight = width * 0.73
            ZStack {
                etchedSignal
                    .frame(width: width + 28, height: width + 28)
                VStack(spacing: 0) {
                    ZStack {
                        RoundedRectangle(cornerRadius: width * 0.19, style: .continuous)
                            .fill(LinearGradient(stops: [
                                .init(color: Color(red: 0.38, green: 0.39, blue: 0.42), location: 0),
                                .init(color: Color(red: 0.25, green: 0.26, blue: 0.28), location: 0.46),
                                .init(color: Color(red: 0.16, green: 0.17, blue: 0.19), location: 1)
                            ], startPoint: .topLeading, endPoint: .bottomTrailing))
                        RoundedRectangle(cornerRadius: width * 0.19, style: .continuous)
                            .strokeBorder(LinearGradient(colors: [.white.opacity(0.3), .white.opacity(0.03), .black.opacity(0.45)],
                                                         startPoint: .topLeading, endPoint: .bottomTrailing), lineWidth: 1)
                        if let icon = NSApp.applicationIconImage {
                            Image(nsImage: icon).resizable().interpolation(.high).scaledToFit()
                                .frame(width: width * 0.40, height: width * 0.40)
                                .opacity(isOnline ? 1 : 0.6)
                                .shadow(color: .black.opacity(0.25), radius: 8, y: 5)
                        }
                    }
                    .frame(width: width, height: enclosureHeight)
                    .rotation3DEffect(.degrees(34), axis: (x: 1, y: 0, z: 0), perspective: 0.4)
                    .zIndex(1)
                    frontEdge(width: width)
                        .frame(height: 34)
                        .offset(y: -25)
                        .padding(.bottom, -25)
                }
                .shadow(color: .black.opacity(0.32), radius: 16, y: 18)
                .offset(y: -6)
                Ellipse().fill(.black.opacity(0.20))
                    .frame(width: width * 0.8, height: 12)
                    .blur(radius: 9)
                    .offset(y: enclosureHeight * 0.57)
                    .zIndex(-1)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    private var etchedSignal: some View {
        ZStack {
            ForEach(0..<3, id: \.self) { index in
                RoundedRectangle(cornerRadius: 38 + CGFloat(index) * 8, style: .continuous)
                    .stroke(MelloTVTheme.border.opacity(0.8 - Double(index) * 0.2), lineWidth: 0.7)
                    .padding(CGFloat(2 - index) * 18)
                    .rotationEffect(.degrees(-9))
            }
        }
        .rotation3DEffect(.degrees(20), axis: (x: 1, y: 0, z: 0))
        .offset(y: -9)
    }

    private func frontEdge(width: CGFloat) -> some View {
        ZStack {
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .fill(LinearGradient(colors: [Color(red: 0.18, green: 0.19, blue: 0.21), Color(red: 0.11, green: 0.12, blue: 0.14)],
                                     startPoint: .top, endPoint: .bottom))
                .overlay { RoundedRectangle(cornerRadius: 18).strokeBorder(.white.opacity(0.10), lineWidth: 0.7) }
            HStack {
                HStack(spacing: 3) {
                    ForEach(0..<9, id: \.self) { _ in
                        Capsule().fill(.black.opacity(0.5)).frame(width: 2, height: 7)
                    }
                }
                Spacer()
                Capsule()
                    .fill(needsAttention ? Color.orange : isOnline ? MelloTVTheme.accent : .white.opacity(0.15))
                    .frame(width: 12, height: 3)
            }
            .padding(.horizontal, 24)
        }
        .frame(width: width * 0.94)
    }
}
