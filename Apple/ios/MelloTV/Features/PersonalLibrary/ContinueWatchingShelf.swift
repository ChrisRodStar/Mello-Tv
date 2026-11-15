import SwiftUI
import MelloKit

struct ContinueWatchingShelf: View {
    let items: [WatchProgress]
    var showsHeading = true
    var resolvingTitleID: String? = nil
    var isUpdating = false
    var onPlay: (WatchProgress) -> Void
    var onInfo: (MediaTitle) -> Void
    var onUpdate: (WatchProgress, ContinueWatchingAction) -> Void

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
                                isPlaybackDisabled: resolvingTitleID != nil || isUpdating,
                                onPlay: { onPlay(item) },
                                onInfo: { onInfo(item.title) },
                                onUpdate: { onUpdate(item, $0) }
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

struct ContinueWatchingMenu<Content: View>: View {
    let item: WatchProgress
    let isDisabled: Bool
    let onInfo: () -> Void
    let onUpdate: (ContinueWatchingAction) -> Void
    @ViewBuilder let label: () -> Content

    var body: some View {
        Menu {
            Button(action: onInfo) {
                Label("Title details", systemImage: "info.circle")
            }
            if !item.completed {
                Button { onUpdate(.watched) } label: {
                    Label(
                        item.title.type == .tv ? "Mark episode as watched" : "Mark as watched",
                        systemImage: "checkmark.circle"
                    )
                }
            }
            Button { onUpdate(.remove) } label: {
                Label("Remove from Continue Watching", systemImage: "minus.circle")
            }
        } label: {
            label()
        }
        .buttonStyle(.plain)
        .disabled(isDisabled)
        .accessibilityLabel("Options for \(item.title.title)")
    }
}

extension View {
    func continueWatchingErrorAlert(_ libraryStore: LibraryStore) -> some View {
        @Bindable var libraryStore = libraryStore
        return alert("Couldn’t update Continue Watching", isPresented: $libraryStore.continueWatchingError) {
            Button("OK", role: .cancel) {}
        } message: {
            Text("Try again in a moment.")
        }
    }
}
