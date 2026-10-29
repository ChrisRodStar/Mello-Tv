import XCTest
import SwiftUI
@testable import MelloKit

final class MelloKitTests: XCTestCase {
    func testAppLinks() {
        XCTAssertEqual(AppLinks.websiteURL.absoluteString, "https://mellowatch.com")
        XCTAssertEqual(AppLinks.clientURL(port: 8080).absoluteString, "http://127.0.0.1:8080/")
        XCTAssertEqual(AppLinks.sourceCodeURL, AppLinks.githubURL)
    }

    func testServerSnapshotDecoding() throws {
        let json = """
        {
            "app_version": "0.1.0",
            "catalog_snapshot": "retained",
            "catalog_status": "ready",
            "uptime_seconds": 120,
            "endpoints": ["http://127.0.0.1:8080"],
            "media": {
                "configured": true,
                "providers": [
                    {
                        "id": "vidcore",
                        "version": "1.0",
                        "last_observed_at": 1700000000.0,
                        "last_failure": null,
                        "candidate_count": 5
                    }
                ]
            },
            "provider_updates": {
                "mode": "automatic",
                "available": true
            }
        }
        """.data(using: .utf8)!

        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let snapshot = try decoder.decode(ServerSnapshot.self, from: json)

        XCTAssertEqual(snapshot.appVersion, "0.1.0")
        XCTAssertEqual(snapshot.uptimeSeconds, 120)
        XCTAssertTrue(snapshot.media.configured)
        XCTAssertEqual(snapshot.media.providers.count, 1)

        let provider = snapshot.media.providers[0]
        XCTAssertEqual(provider.name, "VidCore")
        XCTAssertEqual(provider.activity, "Sources returned")
    }

    func testMediaTitleDecoding() throws {
        let json = """
        {
            "id": 12345,
            "type": "movie",
            "title": "Inception",
            "overview": "A dream within a dream",
            "year": "2010",
            "rating": 8.8,
            "backdropUrl": "https://image.tmdb.org/t/p/w500/test.jpg",
            "posterUrl": "https://image.tmdb.org/t/p/w500/poster.jpg",
            "genres": ["Action", "Sci-Fi"]
        }
        """.data(using: .utf8)!

        let decoder = JSONDecoder()
        let title = try decoder.decode(MediaTitle.self, from: json)

        XCTAssertEqual(title.tmdbID, 12345)
        XCTAssertEqual(title.type, .movie)
        XCTAssertEqual(title.title, "Inception")
        XCTAssertEqual(title.id, "movie-12345")
        XCTAssertEqual(title.genres, ["Action", "Sci-Fi"])
    }

    func testColorHexParsing() {
        #if canImport(AppKit)
        let red = NSColor(hex: "#FF0000")
        XCTAssertEqual(red.redComponent, 1.0, accuracy: 0.01)
        XCTAssertEqual(red.greenComponent, 0.0, accuracy: 0.01)
        XCTAssertEqual(red.blueComponent, 0.0, accuracy: 0.01)

        let green = NSColor(hex: "00FF00")
        XCTAssertEqual(green.redComponent, 0.0, accuracy: 0.01)
        XCTAssertEqual(green.greenComponent, 1.0, accuracy: 0.01)
        XCTAssertEqual(green.blueComponent, 0.0, accuracy: 0.01)
        #endif

        let profileColor = ProfileColor.color(for: "#446e88")
        XCTAssertNotNil(profileColor)
        XCTAssertEqual(ProfileColor.name(for: "#446e88"), "Blue")
    }

    func testAppearanceMode() {
        XCTAssertEqual(AppearanceMode.allCases.count, 3)
        XCTAssertNil(AppearanceMode.system.colorScheme)
        XCTAssertEqual(AppearanceMode.light.colorScheme, .light)
        XCTAssertEqual(AppearanceMode.dark.colorScheme, .dark)

        // Standardized Phosphor icon names for both platforms
        XCTAssertEqual(AppearanceMode.system.iconName, "circle-half")
        XCTAssertEqual(AppearanceMode.light.iconName, "sun")
        XCTAssertEqual(AppearanceMode.dark.iconName, "moon")
    }
}
