import MelloKit
import SwiftUI

/// A single, horizontally scrollable category strip leaves the artwork room to breathe.
struct RecommendationCategoryPicker: View {
    let categories: [RecommendationChoice]
    let selected: RecommendationChoice
    let counts: [RecommendationChoice: Int]
    var onSelect: (RecommendationChoice) -> Void

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView(.horizontal) {
                HStack(spacing: 24) {
                    ForEach(categories) { category in
                        categoryButton(category).id(category)
                    }
                }
                .padding(.horizontal, 20)
            }
            .scrollIndicators(.hidden)
            .onChange(of: selected) { _, category in
                withAnimation(.easeInOut(duration: 0.2)) {
                    proxy.scrollTo(category, anchor: .center)
                }
            }
        }
        .overlay(alignment: .bottom) {
            Rectangle().fill(MelloTVTheme.subtleBorder).frame(height: 0.5)
        }
    }

    private func categoryButton(_ category: RecommendationChoice) -> some View {
        Button {
            onSelect(category)
        } label: {
            HStack(spacing: 6) {
                Text(label(for: category))
                    .font(.subheadline.weight(category == selected ? .semibold : .medium))
                Text((counts[category] ?? 0).formatted())
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(MelloTVTheme.textSecondary)
            }
            .foregroundStyle(category == selected ? MelloTVTheme.text : MelloTVTheme.textSecondary)
            .fixedSize()
            .padding(.vertical, 16)
            .frame(minHeight: 48)
            .overlay(alignment: .bottom) {
                if category == selected {
                    Capsule().fill(MelloTVTheme.text).frame(height: 2)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(category == selected ? .isSelected : [])
        .accessibilityLabel(label(for: category))
        .accessibilityValue("\(counts[category] ?? 0) titles")
    }

    private func label(for category: RecommendationChoice) -> String {
        switch category {
        case .favorites: "Liked"
        case .hidden: "Not for me"
        case .watched: "Watched"
        case .ignored: "Excluded from taste"
        }
    }
}
