import SwiftUI

struct SettingsLibrarySection: View {
    let profileID: String
    let libraryStore: LibraryStore

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            SettingsGroup(title: "Library") {
                ForEach(LibraryCollection.allCases) { collection in
                    VStack(spacing: 0) {
                        if collection != .continueWatching { SettingsRowDivider() }
                        NavigationLink(value: collection) {
                            SettingsRow(
                                title: collection.title,
                                value: libraryStore.isLoading ? nil : String(count(for: collection))
                            )
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
            if let message = libraryStore.errorMessage {
                Text(message)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                Button("Try again") {
                    Task { await libraryStore.refresh(for: profileID) }
                }
                .font(.subheadline.weight(.medium))
                .frame(minHeight: 44)
            }
        }
    }

    private func count(for collection: LibraryCollection) -> Int {
        switch collection {
        case .continueWatching: libraryStore.continueWatching().count
        case .myList: libraryStore.library.list.count
        case .history: libraryStore.watchHistory.count
        }
    }
}
