import MelloKit
import SwiftUI

struct RecommendationChoicePoster: View {
    let title: MediaTitle
    let isEditing: Bool
    let isSelected: Bool
    var onTap: () -> Void
    var onSelect: () -> Void

    var body: some View {
        Button(action: onTap) {
            VStack(alignment: .leading, spacing: 8) {
                Rectangle()
                    .fill(MelloTVTheme.secondaryBackground)
                    .aspectRatio(2.0 / 3.0, contentMode: .fit)
                    .overlay {
                        AsyncImage(url: title.posterURL) { phase in
                            if case .success(let image) = phase {
                                image.resizable().scaledToFill()
                            }
                        }
                    }
                    .clipped()
                    .clipShape(.rect(cornerRadius: 12))
                    .overlay {
                        RoundedRectangle(cornerRadius: 12)
                            .strokeBorder(
                                isSelected ? MelloTVTheme.accent : MelloTVTheme.subtleBorder,
                                lineWidth: isSelected ? 3 : 0.5)
                    }
                    .overlay(alignment: .bottomTrailing) {
                        if isEditing {
                            Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                                .font(.system(size: 24, weight: .medium))
                                .foregroundStyle(isSelected ? MelloTVTheme.accent : .white)
                                .background(.black.opacity(0.65), in: Circle())
                                .padding(8)
                        }
                    }
                Text(title.title)
                    .font(.caption.weight(.medium))
                    .foregroundStyle(MelloTVTheme.text)
                    .lineLimit(2, reservesSpace: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(CardScaleButtonStyle(scale: 0.98))
        .highPriorityGesture(
            LongPressGesture(minimumDuration: 0.45)
                .onEnded { _ in onSelect() }
        )
        .accessibilityAction(named: Text("Select for removal"), onSelect)
        .accessibilityLabel(title.title)
        .accessibilityHint(
            isEditing
                ? (isSelected ? "Deselect title" : "Select for removal") : "Shows title details"
        )
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}
