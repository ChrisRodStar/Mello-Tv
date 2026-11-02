import MelloKit
import SwiftUI

struct LibraryProgressPages: View {
    let items: [WatchProgress]
    let resolvingItem: LibraryProgressID?
    var showsDates = false
    var onPlay: (WatchProgress) -> Void
    var onInfo: (MediaTitle) -> Void

    @ScaledMetric(relativeTo: .body) private var rowHeight = 100
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    var body: some View {
        LibraryPager(
            items: items,
            id: \.libraryID,
            minimumCellSize: CGSize(
                width: dynamicTypeSize.isAccessibilitySize ? 600 : 310,
                height: rowHeight * (showsDates ? 1.15 : 1)),
            maximumColumns: dynamicTypeSize.isAccessibilitySize ? 1 : 2
        ) { item in
            LibraryProgressRow(
                item: item,
                showsDate: showsDates,
                isPreparingPlayback: resolvingItem == item.libraryID,
                isPlaybackDisabled: resolvingItem != nil,
                onPlay: { onPlay(item) },
                onInfo: { onInfo(item.title) }
            )
        }
    }
}
