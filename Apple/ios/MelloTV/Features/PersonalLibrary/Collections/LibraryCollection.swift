import Foundation

enum LibraryCollection: String, CaseIterable, Identifiable, Hashable {
    case continueWatching
    case myList
    case history

    var id: Self { self }

    var title: String {
        switch self {
        case .continueWatching: "Continue Watching"
        case .myList: "My List"
        case .history: "History"
        }
    }

    var systemImage: String {
        switch self {
        case .continueWatching: "play.circle"
        case .myList: "bookmark"
        case .history: "clock.arrow.circlepath"
        }
    }

    var emptyTitle: String {
        switch self {
        case .continueWatching: "Ready when you are"
        case .myList: "Save something for later"
        case .history: "Your viewing starts here"
        }
    }

    var emptyMessage: String {
        switch self {
        case .continueWatching: "Pick a movie or series. You can return to it here."
        case .myList: "Tap the bookmark on any title to add it to My List."
        case .history: "The movies and episodes you watch will appear here."
        }
    }
}
