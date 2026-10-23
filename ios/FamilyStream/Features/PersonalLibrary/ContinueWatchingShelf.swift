import SwiftUI

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
                        .foregroundStyle(FamilyStreamTheme.text)
                        .accessibilityAddTraits(.isHeader)
                        .padding(.horizontal, 16)
                }
                ScrollView(.horizontal, showsIndicators: false) {
                    LazyHStack(alignment: .top, spacing: 14) {
                        ForEach(items, id: \.title.id) { item in
                            LandscapeCard(
                                item: item,
                                isPreparingPlayback: resolvingTitleID == item.title.id,
                                isPlaybackDisabled: resolvingTitleID != nil,
                                onPlay: { onPlay(item) },
                                onInfo: { onInfo(item.title) }
                            )
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 4)
                }
                .scrollClipDisabled()
            }
        }
    }
}
