import Foundation

enum OnboardingPoster: String, CaseIterable, Identifiable {
    case sintel
    case bigBuckBunny = "big-buck-bunny"
    case elephantsDream = "elephants-dream"

    var id: Self { self }

    var title: String {
        switch self {
        case .sintel: "Sintel"
        case .bigBuckBunny: "Big Buck Bunny"
        case .elephantsDream: "Elephants Dream"
        }
    }

    var credit: String {
        switch self {
        case .sintel: "© Blender Foundation | durian.blender.org"
        case .bigBuckBunny: "© Blender Foundation | www.bigbuckbunny.org"
        case .elephantsDream: "Bassam Kurdali, Andy Goralczyk, and the Blender Foundation"
        }
    }

    var licenseVersion: String { self == .elephantsDream ? "4.0" : "3.0" }

    var source: URL {
        switch self {
        case .sintel: URL(string: "https://commons.wikimedia.org/wiki/File:Sintel_poster.jpg")!
        case .bigBuckBunny:
            URL(string: "https://commons.wikimedia.org/wiki/File:Big_buck_bunny_poster_big.jpg")!
        case .elephantsDream:
            URL(string: "https://commons.wikimedia.org/wiki/File:ElephantsDreamPoster.jpg")!
        }
    }
}
