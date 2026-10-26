import Cocoa

/// Window bounds live here, separate from individual SwiftUI component sizes.
enum WindowSizing {
    /// Whole window, including the title bar and toolbar. Both opening and minimum size.
    static let mainFrame = NSSize(width: 1150, height: 750)

    /// Onboarding uses content dimensions, excluding window chrome.
    static let welcomeContent = NSSize(width: 560, height: 640)
    static let welcomeMinimumContent = NSSize(width: 360, height: 400)

    static func constrainedMainFrame(_ proposed: NSSize) -> NSSize {
        NSSize(width: max(mainFrame.width, proposed.width),
               height: max(mainFrame.height, proposed.height))
    }
}
