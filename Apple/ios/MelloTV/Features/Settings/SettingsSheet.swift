import Foundation

/// Modal editors own their save/cancel lifecycle independently of tab navigation.
enum SettingsSheet: String, Identifiable {
    case playback
    case recommendations

    var id: Self { self }
}
