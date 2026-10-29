import Cocoa
import SwiftUI

/// AppKit owns the window bounds; SwiftUI fills the available content area.
/// Keep hosting below a plain container so it cannot propagate root hosting
/// controller minimum/maximum sizes back to NSWindow when destinations change.
@MainActor
final class WindowHostingController<Content: View>: NSViewController {
    private let hostingView: NSHostingView<Content>

    init(rootView: Content) {
        hostingView = NSHostingView(rootView: rootView)
        hostingView.sizingOptions = []
        super.init(nibName: nil, bundle: nil)
    }

    required init?(coder: NSCoder) { nil }

    override func loadView() {
        let container = NSView()
        view = container
        hostingView.translatesAutoresizingMaskIntoConstraints = false
        container.addSubview(hostingView)
        NSLayoutConstraint.activate([
            hostingView.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            hostingView.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            hostingView.topAnchor.constraint(equalTo: container.topAnchor),
            hostingView.bottomAnchor.constraint(equalTo: container.bottomAnchor)
        ])
    }
}

typealias WindowContentController = WindowHostingController

