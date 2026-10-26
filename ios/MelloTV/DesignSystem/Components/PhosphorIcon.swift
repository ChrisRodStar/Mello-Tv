import SwiftUI
#if canImport(UIKit)
import UIKit
#endif

public enum PhosphorWeight: String, Sendable {
    case regular
    case bold
    case fill
}

public struct PhosphorIcon: View {
    let name: String
    var size: CGFloat
    var weight: PhosphorWeight

    public init(_ name: String, size: CGFloat = 20, weight: PhosphorWeight = .regular) {
        self.name = name
        self.size = size
        self.weight = weight
    }

    public var body: some View {
        let assetName = weight == .regular ? name : "\(name)-\(weight.rawValue)"
        Image(assetName)
            .renderingMode(.template)
            .resizable()
            .scaledToFit()
            .frame(width: size, height: size)
    }
}

#if canImport(UIKit)
public extension UIImage {
    static func phosphor(_ name: String, weight: PhosphorWeight = .regular) -> UIImage? {
        let assetName = weight == .regular ? name : "\(name)-\(weight.rawValue)"
        return UIImage(named: assetName)?.withRenderingMode(.alwaysTemplate)
    }
}
#endif
