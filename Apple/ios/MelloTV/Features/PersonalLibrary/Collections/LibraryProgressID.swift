import MelloKit

/// Episodes of the same series must remain distinct in history and playback state.
struct LibraryProgressID: Hashable {
    let titleID: String
    let season: Int
    let episode: Int
}

extension WatchProgress {
    var libraryID: LibraryProgressID {
        LibraryProgressID(titleID: title.id, season: season, episode: episode)
    }
}
