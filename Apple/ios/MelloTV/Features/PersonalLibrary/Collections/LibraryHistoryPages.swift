import MelloKit
import SwiftUI

struct LibraryHistoryPages: View {
    let items: [WatchProgress]
    let resolvingItem: LibraryProgressID?
    var onPlay: (WatchProgress) -> Void
    var onInfo: (MediaTitle) -> Void

    private var newestFirst: [WatchProgress] {
        items.sorted {
            ($0.updatedAt.isFinite ? $0.updatedAt : 0) > ($1.updatedAt.isFinite ? $1.updatedAt : 0)
        }
    }

    var body: some View {
        LibraryProgressPages(
            items: newestFirst,
            resolvingItem: resolvingItem,
            showsDates: true,
            onPlay: onPlay,
            onInfo: onInfo
        )
    }
}
