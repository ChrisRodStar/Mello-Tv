import MelloKit
import SwiftUI

struct RecommendationPosterGrid: View {
    let titles: [MediaTitle]
    let isEditing: Bool
    let selectedIDs: Set<String>
    var onTap: (MediaTitle) -> Void
    var onSelect: (MediaTitle) -> Void

    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    var body: some View {
        LazyVGrid(
            columns: [
                GridItem(
                    .adaptive(
                        minimum: dynamicTypeSize.isAccessibilitySize ? 150 : 100,
                        maximum: 180), spacing: 12, alignment: .top)
            ],
            alignment: .leading,
            spacing: 20
        ) {
            ForEach(titles) { title in
                RecommendationChoicePoster(
                    title: title,
                    isEditing: isEditing,
                    isSelected: selectedIDs.contains(title.id),
                    onTap: { onTap(title) },
                    onSelect: { onSelect(title) }
                )
            }
        }
    }
}
