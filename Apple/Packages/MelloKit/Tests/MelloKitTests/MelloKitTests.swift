import XCTest
import SwiftUI
#if os(macOS)
import AppKit
#endif
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

        // Bundled Phosphor asset names used by iOS
        XCTAssertEqual(AppearanceMode.system.iconName, "circle-half")
        XCTAssertEqual(AppearanceMode.light.iconName, "sun")
        XCTAssertEqual(AppearanceMode.dark.iconName, "moon")
    }

    #if os(macOS)
    func testAppearanceSystemImagesExist() {
        for mode in AppearanceMode.allCases {
            XCTAssertNotNil(
                NSImage(systemSymbolName: mode.systemImageName, accessibilityDescription: nil),
                "Missing SF Symbol for \(mode.rawValue): \(mode.systemImageName)"
            )
        }
    }
    #endif

    func testThemeTokens() {
        // Verify iOS-standardized surfaces and tokens
        XCTAssertNotNil(MelloTVTheme.background)
        XCTAssertNotNil(MelloTVTheme.card)
        XCTAssertNotNil(MelloTVTheme.cardBackground)
        XCTAssertNotNil(MelloTVTheme.border)
        XCTAssertNotNil(MelloTVTheme.text)
        XCTAssertNotNil(MelloTVTheme.accent)
        XCTAssertNotNil(MelloTVTheme.accentGlow)
    }

    func testUpdateReadinessDecoding() throws {
        let json = #"{"safe_to_restart": true}"#.data(using: .utf8)!
        let readiness = try JSONDecoder().decode(UpdateReadiness.self, from: json)
        XCTAssertTrue(readiness.safeToRestart)

        let blockedJson = #"{"safe_to_restart": false}"#.data(using: .utf8)!
        let blocked = try JSONDecoder().decode(UpdateReadiness.self, from: blockedJson)
        XCTAssertFalse(blocked.safeToRestart)
    }

    func testProfilePersistence() {
        let url = URL(string: "http://192.168.1.50:8080")!
        let expectedKey = "family-stream.active-profile.v3.192.168.1.50:8080"
        XCTAssertEqual(ProfilePersistence.storageKey(for: url), expectedKey)

        ProfilePersistence.setActiveProfileID("test-profile-123", for: url)
        XCTAssertEqual(ProfilePersistence.activeProfileID(for: url), "test-profile-123")

        ProfilePersistence.setActiveProfileID(nil, for: url)
        XCTAssertNil(ProfilePersistence.activeProfileID(for: url))
    }

    func testAPIError() {
        let busyError = APIError.service(status: 503, code: "playback_busy", message: "Playback is busy")
        XCTAssertTrue(busyError.isRetryablePlayback)
        XCTAssertFalse(busyError.isExpiredPlayback)

        let expiredError = APIError.service(status: 401, code: "session_expired", message: "Session expired")
        XCTAssertTrue(expiredError.isExpiredPlayback)
        XCTAssertFalse(expiredError.isRetryablePlayback)

        let connError = APIError.connection("Failed to reach server")
        XCTAssertTrue(connError.isRetryablePlayback)
        XCTAssertEqual(connError.errorDescription, "Failed to reach server")

        let notFoundError = APIError.serverStatus(404)
        XCTAssertEqual(notFoundError.errorDescription, "This item isn’t cached yet. Try again shortly.")
    }

    func testTimeFormatting() {
        // Uptime
        XCTAssertEqual(TimeFormatting.formatUptime(45), "45s")
        XCTAssertEqual(TimeFormatting.formatUptime(125), "2m 5s")
        XCTAssertEqual(TimeFormatting.formatUptime(3665), "1h 1m")
        XCTAssertEqual(TimeFormatting.formatUptime(90000), "1d 1h")

        // Duration
        XCTAssertEqual(TimeFormatting.formatDuration(0), "0:00")
        XCTAssertEqual(TimeFormatting.formatDuration(65), "1:05")
        XCTAssertEqual(TimeFormatting.formatDuration(3665), "1:01:05")

        // Remaining
        XCTAssertEqual(TimeFormatting.formatRemaining(position: 100, duration: 200), "2m")
        XCTAssertEqual(TimeFormatting.formatRemaining(position: 200, duration: 200), "<1m")
        XCTAssertEqual(TimeFormatting.formatRemaining(position: 0, duration: 7200), "2h 0m")
    }

    func testPhosphorWeights() {
        XCTAssertEqual(PhosphorWeight.allCases.count, 3)
        XCTAssertEqual(PhosphorWeight.regular.rawValue, "regular")
        XCTAssertEqual(PhosphorWeight.bold.rawValue, "bold")
        XCTAssertEqual(PhosphorWeight.fill.rawValue, "fill")
    }
}
