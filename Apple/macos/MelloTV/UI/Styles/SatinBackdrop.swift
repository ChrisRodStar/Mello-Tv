import AppKit
import SwiftUI
import simd

/// A cached rendering of the satin field with Titanium White / Obsidian textures.
struct SatinBackdrop: View {
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        GeometryReader { geometry in
            Image(nsImage: colorScheme == .dark ? Self.darkTexture : Self.lightTexture)
                .resizable()
                .scaledToFill()
                .frame(width: geometry.size.width, height: geometry.size.height)
                .clipped()
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    private static let darkTexture: NSImage = generateTexture(isDark: true)
    private static let lightTexture: NSImage = generateTexture(isDark: false)

    private static func generateTexture(isDark: Bool) -> NSImage {
        let width = 576
        let height = 384
        var pixels = [UInt8](repeating: 255, count: width * height * 4)
        let light = simd_normalize(SIMD3<Float>(-0.45, -0.35, 1))
        for y in 0..<height {
            for x in 0..<width {
                let position = SIMD2<Float>(Float(x) * 2, Float(y) * 2)
                var p = position / 170
                p += SIMD2<Float>(noise(p * 0.7 + 4.3), noise(p * 0.7 + 19.1)) * 1.3
                let value = field(p)
                let dx = field(p + SIMD2<Float>(0.035, 0)) - value
                let dy = field(p + SIMD2<Float>(0, 0.035)) - value
                let normal = simd_normalize(SIMD3<Float>(-dx * 14, -dy * 14, 1))
                let diffuse = simd_dot(normal, light)
                let reflection = 2 * simd_dot(normal, light) * normal - light
                let sheen = pow(max(0, reflection.z), 12)
                let uv = SIMD2<Float>(Float(x) / Float(width), Float(y) / Float(height))
                let d = uv - SIMD2<Float>(0.34, 0.28)
                let atmosphere = exp(-(d.x * d.x * 3 + d.y * d.y * 3.6))
                let pigment = smoothstep(0.2, 0.8, value)

                // Dark: #121316 base with obsidian tone
                // Light: #F2F2F7 base with pure crisp titanium white sheen
                let base = isDark
                    ? SIMD3<Float>(18 / 255, 19 / 255, 22 / 255)
                    : SIMD3<Float>(242 / 255, 242 / 255, 247 / 255)
                let cool = isDark
                    ? SIMD3<Float>(0.10, 0.19, 0.27)
                    : SIMD3<Float>(0.96, 0.97, 1.0)

                var result = base + (cool - base) * ((0.18 + pigment * 0.45) * 0.52)
                let grain = (hash(position * 2) - 0.5) * (isDark ? 0.034 : 0.012)
                result += SIMD3<Float>(repeating: (value - 0.5) * (isDark ? 0.095 : 0.025) + grain + (diffuse - 0.82) * (isDark ? 0.14 : 0.04))
                result += sheen * atmosphere * (isDark ? 0.08 : 0.025) * SIMD3<Float>(0.9, 0.95, 1)

                let index = (y * width + x) * 4
                for channel in 0..<3 {
                    pixels[index + channel] = UInt8(max(0, min(255, result[channel] * 255)))
                }
            }
        }

        let colorSpace = CGColorSpaceCreateDeviceRGB()
        let bitmapInfo = CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue)
        guard let provider = CGDataProvider(data: Data(pixels) as CFData),
              let cgImage = CGImage(width: width, height: height, bitsPerComponent: 8,
                                    bitsPerPixel: 32, bytesPerRow: width * 4,
                                    space: colorSpace, bitmapInfo: bitmapInfo,
                                    provider: provider, decode: nil,
                                    shouldInterpolate: true, intent: .defaultIntent) else {
            return NSImage()
        }
        return NSImage(cgImage: cgImage, size: NSSize(width: width, height: height))
    }

    private static func field(_ p: SIMD2<Float>) -> Float {
        var value: Float = 0
        var amplitude: Float = 0.55
        var current = p
        for _ in 0..<3 {
            value += amplitude * (sin(current.x) * cos(current.y))
            current = simd_mul(simd_float2x2(rows: [
                SIMD2<Float>(1.6, 1.2),
                SIMD2<Float>(-1.2, 1.6)
            ]), current)
            amplitude *= 0.5
        }
        return value * 0.5 + 0.5
    }

    private static func noise(_ p: SIMD2<Float>) -> Float {
        let i = SIMD2<Float>(floor(p.x), floor(p.y))
        let f = p - i
        let u = f * f * (SIMD2<Float>(3, 3) - 2 * f)
        let a = hash(i)
        let b = hash(i + SIMD2<Float>(1, 0))
        let c = hash(i + SIMD2<Float>(0, 1))
        let d = hash(i + SIMD2<Float>(1, 1))
        return simd_mix(simd_mix(a, b, u.x), simd_mix(c, d, u.x), u.y)
    }

    private static func hash(_ p: SIMD2<Float>) -> Float {
        let q = SIMD2<Float>(
            simd_dot(p, SIMD2<Float>(127.1, 311.7)),
            simd_dot(p, SIMD2<Float>(269.5, 183.3))
        )
        return fract(sin(q.x) * 43758.5453)
    }

    private static func fract(_ v: Float) -> Float { v - floor(v) }
    private static func smoothstep(_ edge0: Float, _ edge1: Float, _ x: Float) -> Float {
        let t = max(0, min(1, (x - edge0) / (edge1 - edge0)))
        return t * t * (3 - 2 * t)
    }
}
