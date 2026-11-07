import SwiftUI

public struct BrandTVMark: View {
    public init() {}
    public var body: some View {
        HStack(spacing: 3) {
            Image("brand-tv-mark")
                .resizable()
                .scaledToFit()
                .frame(width: 14, height: 13)
            Text("TV")
                .font(.system(size: 11, weight: .bold))
                .tracking(-0.2)
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 3)
        .background(.ultraThinMaterial, in: Capsule())
        .environment(\.colorScheme, .dark)
        .background(Color.black.opacity(0.35), in: Capsule())
        .foregroundStyle(.white.opacity(0.85))
        .shadow(color: .black.opacity(0.4), radius: 2, y: 1)
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

public struct CardDetailsGlyph: View {
    public init() {}
    public var body: some View {
        Image(systemName: "ellipsis")
            .font(.system(size: 16, weight: .semibold))
            .foregroundStyle(.white)
            .frame(width: 36, height: 36)
            .background(.ultraThinMaterial, in: Circle())
            .environment(\.colorScheme, .dark)
            .background(Color.black.opacity(0.35), in: Circle())
            .overlay {
                Circle().strokeBorder(Color.white.opacity(0.20), lineWidth: 0.75)
            }
            .contentShape(Rectangle())
    }
}
