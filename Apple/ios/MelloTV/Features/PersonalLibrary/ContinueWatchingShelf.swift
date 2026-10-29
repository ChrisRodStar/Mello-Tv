import SwiftUI
import MelloKit

struct ContinueWatchingShelf: View {
    let items: [WatchProgress]
    var showsHeading = true
    var resolvingTitleID: String? = nil
    var onPlay: (WatchProgress) -> Void
    var onInfo: (MediaTitle) -> Void

    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    var body: some View {
        if !items.isEmpty {
            VStack(alignment: .leading, spacing: 12) {
                if showsHeading {
                    Text("Continue Watching")
                        .font(.title3.weight(.bold))
                        .foregroundStyle(MelloTVTheme.text)
                        .accessibilityAddTraits(.isHeader)
                        .padding(.horizontal, 16)
                }
                ScrollView(.horizontal, showsIndicators: false) {
                    LazyHStack(alignment: .top, spacing: 14) {
                        ForEach(items, id: \.title.id) { item in
                            ContinueWatchingCard(
                                item: item,
                                isPreparingPlayback: resolvingTitleID == item.title.id,
                                isPlaybackDisabled: resolvingTitleID != nil,
                                onPlay: { onPlay(item) },
                                onInfo: { onInfo(item.title) }
                            )
                            .containerRelativeFrame(.horizontal) { width, _ in
                                min(360, max(220, width * 0.72))
                            }
                        }
                    }
                    .scrollTargetLayout()
                    .padding(.horizontal, 16)
                    .padding(.vertical, 4)
                }
                .scrollTargetBehavior(.viewAligned)
                .scrollClipDisabled()
            }
        }
    }
}
