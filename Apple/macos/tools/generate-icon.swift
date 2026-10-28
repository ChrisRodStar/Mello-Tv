import Cocoa

let iconsetPath = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "Stream.iconset"
let outputPath = CommandLine.arguments.count > 2 ? CommandLine.arguments[2] : "AppIcon.icns"

func drawIcon(size: CGFloat) -> NSImage {
    let img = NSImage(size: NSSize(width: size, height: size))
    img.lockFocus()
    
    let rect = NSRect(x: 0, y: 0, width: size, height: size)
    
    // Background squircle
    let cornerRadius = size * 0.22
    let bgPath = NSBezierPath(roundedRect: rect.insetBy(dx: size * 0.04, dy: size * 0.04), xRadius: cornerRadius, yRadius: cornerRadius)
    
    // Dark metallic gradient background
    let gradient = NSGradient(colors: [
        NSColor(red: 0.13, green: 0.14, blue: 0.17, alpha: 1.0),
        NSColor(red: 0.08, green: 0.09, blue: 0.11, alpha: 1.0)
    ])!
    gradient.draw(in: bgPath, angle: -45)
    
    // Subtle border
    NSColor(red: 0.22, green: 0.24, blue: 0.28, alpha: 1.0).setStroke()
    bgPath.lineWidth = size * 0.02
    bgPath.stroke()
    
    // TV frame
    let tvWidth = size * 0.60
    let tvHeight = size * 0.44
    let tvRect = NSRect(x: (size - tvWidth) / 2.0, y: size * 0.32, width: tvWidth, height: tvHeight)
    let tvPath = NSBezierPath(roundedRect: tvRect, xRadius: size * 0.06, yRadius: size * 0.06)
    NSColor(red: 0.18, green: 0.19, blue: 0.22, alpha: 1.0).setFill()
    tvPath.fill()
    NSColor(red: 0.30, green: 0.32, blue: 0.36, alpha: 1.0).setStroke()
    tvPath.lineWidth = size * 0.015
    tvPath.stroke()
    
    // TV Screen inner
    let screenRect = tvRect.insetBy(dx: size * 0.03, dy: size * 0.03)
    let screenPath = NSBezierPath(roundedRect: screenRect, xRadius: size * 0.04, yRadius: size * 0.04)
    NSColor(red: 0.05, green: 0.06, blue: 0.08, alpha: 1.0).setFill()
    screenPath.fill()
    
    // Play triangle on screen
    let playPath = NSBezierPath()
    let cx = screenRect.midX
    let cy = screenRect.midY
    let ph = size * 0.14
    let pw = size * 0.13
    playPath.move(to: NSPoint(x: cx - pw * 0.4, y: cy + ph * 0.5))
    playPath.line(to: NSPoint(x: cx + pw * 0.6, y: cy))
    playPath.line(to: NSPoint(x: cx - pw * 0.4, y: cy - ph * 0.5))
    playPath.close()
    
    NSColor(red: 0.95, green: 0.94, blue: 0.92, alpha: 1.0).setFill()
    playPath.fill()
    
    // TV Stand / foot
    let footWidth = size * 0.28
    let footHeight = size * 0.03
    let footRect = NSRect(x: (size - footWidth) / 2.0, y: size * 0.23, width: footWidth, height: footHeight)
    let footPath = NSBezierPath(roundedRect: footRect, xRadius: footHeight / 2.0, yRadius: footHeight / 2.0)
    NSColor(red: 0.35, green: 0.37, blue: 0.42, alpha: 1.0).setFill()
    footPath.fill()
    
    let neckRect = NSRect(x: (size - size * 0.06) / 2.0, y: size * 0.26, width: size * 0.06, height: size * 0.06)
    NSColor(red: 0.28, green: 0.30, blue: 0.34, alpha: 1.0).setFill()
    NSBezierPath(rect: neckRect).fill()
    
    // Glowing Blue LED indicator
    let ledRect = NSRect(x: (size - size * 0.08) / 2.0, y: size * 0.15, width: size * 0.08, height: size * 0.02)
    let ledPath = NSBezierPath(roundedRect: ledRect, xRadius: size * 0.01, yRadius: size * 0.01)
    NSColor(red: 0.16, green: 0.59, blue: 1.0, alpha: 1.0).setFill()
    ledPath.fill()
    
    img.unlockFocus()
    return img
}

func savePNG(image: NSImage, path: String) {
    guard let tiff = image.tiffRepresentation,
          let rep = NSBitmapImageRep(data: tiff),
          let png = rep.representation(using: .png, properties: [:]) else { return }
    try? png.write(to: URL(fileURLWithPath: path))
}

let fm = FileManager.default
try? fm.createDirectory(atPath: iconsetPath, withIntermediateDirectories: true)

let sizes: [(Int, Int, String)] = [
    (16, 1, "icon_16x16.png"),
    (16, 2, "icon_16x16@2x.png"),
    (32, 1, "icon_32x32.png"),
    (32, 2, "icon_32x32@2x.png"),
    (128, 1, "icon_128x128.png"),
    (128, 2, "icon_128x128@2x.png"),
    (256, 1, "icon_256x256.png"),
    (256, 2, "icon_256x256@2x.png"),
    (512, 1, "icon_512x512.png"),
    (512, 2, "icon_512x512@2x.png"),
]

for (pt, scale, name) in sizes {
    let px = CGFloat(pt * scale)
    let img = drawIcon(size: px)
    savePNG(image: img, path: "\(iconsetPath)/\(name)")
}

let proc = Process()
proc.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
proc.arguments = ["-c", "icns", iconsetPath, "-o", outputPath]
try? proc.run()
proc.waitUntilExit()

try? fm.removeItem(atPath: iconsetPath)
print("Successfully generated \(outputPath)")
