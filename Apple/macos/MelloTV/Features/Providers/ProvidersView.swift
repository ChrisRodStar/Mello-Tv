import Combine
import Foundation
import SwiftUI
import MelloKit

@MainActor
struct ProvidersView: View {
    let providers: [ServerSnapshot.ProviderStatus]
    let online: Bool
    let starting: Bool
    let port: Int
    @StateObject private var health = ProviderHealthStore()
    @Environment(\.colorScheme) private var colorScheme

    private var checkIdentity: String {
        online ? providers.map { "\($0.id)@\($0.version)" }.joined(separator: ",") : "offline"
    }

    private var workingCount: Int {
        providers.filter { health.report(for: $0)?.working == true }.count
    }

    private var headline: String {
        if starting { return "Your server is starting." }
        if !online { return "Your server is offline." }
        if providers.isEmpty { return "No providers available." }
        if health.isChecking { return "Checking availability." }
        if health.error != nil { return "Checks are unavailable." }
        if workingCount == providers.count { return "All providers verified." }
        if health.reports.isEmpty { return "Ready to check." }
        return "\(workingCount) of \(providers.count) verified."
    }

    var body: some View {
        GeometryReader { viewport in
            let compact = viewport.size.height < 620
            VStack(alignment: .leading, spacing: compact ? 20 : 26) {
                HStack(alignment: .center, spacing: 24) {
                    VStack(alignment: .leading, spacing: 9) {
                        Text(headline)
                            .font(.system(size: compact ? 27 : 32, weight: .semibold))
                            .tracking(-1)
                            .foregroundStyle(MelloTVTheme.text)
                            .accessibilityAddTraits(.isHeader)
                        Text(starting ? "Checks begin when the server is ready." : online ? "Independent checks of resolution and media access." : "Start your server to check provider availability.")
                            .font(.system(size: 12))
                            .foregroundStyle(MelloTVTheme.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    Spacer(minLength: 0)
                    Button {
                        health.check(providers: providers, port: port, force: true)
                    } label: {
                        HStack(spacing: 8) {
                            if health.isChecking {
                                ProgressView().controlSize(.small)
                            } else {
                                Image(systemName: "arrow.clockwise")
                            }
                            Text(health.isChecking ? "Checking…" : "Check providers")
                        }
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(online ? Color.white : MelloTVTheme.textSecondary)
                        .padding(.horizontal, 17)
                        .frame(height: 40)
                        .background(online ? MelloTVTheme.accent : MelloTVTheme.sunken,
                                    in: RoundedRectangle(cornerRadius: 13, style: .continuous))
                        .overlay {
                            RoundedRectangle(cornerRadius: 13, style: .continuous)
                                .strokeBorder(.white.opacity(online ? 0.16 : 0.04), lineWidth: 1)
                        }
                    }
                    .buttonStyle(OverviewPressStyle())
                    .disabled(!online || health.isChecking || providers.isEmpty)
                    .help("Run independent checks. Results are cached for one minute.")
                }
                .padding(.horizontal, 4)

                VStack(spacing: 0) {
                    if providers.isEmpty {
                        VStack(spacing: 10) {
                            Image(systemName: "play.rectangle")
                                .font(.system(size: 26, weight: .light))
                            Text(starting ? "Waiting for the server to finish starting" : online ? "No providers are available to check" : "Your server is unavailable")
                                .font(.system(size: 13, weight: .medium))
                        }
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .frame(maxWidth: .infinity, minHeight: 260)
                    } else {
                        ForEach(providers) { provider in
                            ProviderHealthRow(
                                provider: provider,
                                report: health.report(for: provider),
                                online: online,
                                checking: health.isChecking,
                                checkFailed: health.error != nil,
                                compact: compact
                            )
                            if provider.id != providers.last?.id {
                                Rectangle().fill(MelloTVTheme.border.opacity(0.55))
                                    .frame(height: 1).padding(.horizontal, 24)
                            }
                        }
                    }
                }
                .padding(.vertical, 8)
                .background {
                    RoundedRectangle(cornerRadius: 28, style: .continuous)
                        .fill(LinearGradient(stops: [
                            .init(color: MelloTVTheme.surfaceHighlight, location: 0),
                            .init(color: MelloTVTheme.card, location: 0.5),
                            .init(color: MelloTVTheme.surfaceShade, location: 1)
                        ], startPoint: .topLeading, endPoint: .bottomTrailing))
                        .overlay {
                            RoundedRectangle(cornerRadius: 28, style: .continuous)
                                .strokeBorder(LinearGradient(
                                    colors: [.white.opacity(colorScheme == .dark ? 0.10 : 0.85), MelloTVTheme.subtleBorder],
                                    startPoint: .topLeading, endPoint: .bottomTrailing
                                ), lineWidth: 1)
                        }
                        .shadow(color: MelloTVTheme.ambientShadow.opacity(colorScheme == .dark ? 0.25 : 0.10),
                                radius: 18, x: 0, y: 12)
                }

                Text(health.error ?? "Checks use known reference titles and verify media access. A failed check does not mean every title is unavailable.")
                    .font(.system(size: 11))
                    .lineSpacing(3)
                    .foregroundStyle(health.error == nil ? MelloTVTheme.textSecondary : Color.orange)
                    .padding(.horizontal, 4)
                    .fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 0)
            }
            .padding(compact ? 26 : 34)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .task(id: checkIdentity) {
            health.cancel()
            guard online else {
                health.reset()
                return
            }
            while !Task.isCancelled {
                health.check(providers: providers, port: port)
                do { try await Task.sleep(nanoseconds: 300_000_000_000) }
                catch { return }
            }
        }
        .onDisappear { health.cancel() }
    }
}

private struct ProviderHealthRow: View {
    let provider: ServerSnapshot.ProviderStatus
    let report: ProviderHealthReport?
    let online: Bool
    let checking: Bool
    let checkFailed: Bool
    let compact: Bool

    private var verified: Bool { online && !checking && !checkFailed && report?.working == true }
    private var failed: Bool { online && !checking && !checkFailed && report?.working == false }
    private var tone: Color { verified ? MelloTVTheme.accent : failed ? .orange : MelloTVTheme.mutedSubtle }
    private var status: String {
        if !online { return "Offline" }
        if checking { return "Checking" }
        if checkFailed { return "Unavailable" }
        guard let report else { return "Not checked" }
        return report.working ? "Working" : "Check failed"
    }
    private var detail: String {
        if !online { return "Start the server to run a check." }
        if checking { return "Resolving reference media and checking access." }
        if checkFailed { return "The health check could not be completed." }
        return report?.detail ?? "No independent check has completed yet."
    }

    var body: some View {
        HStack(spacing: 15) {
            ZStack {
                RoundedRectangle(cornerRadius: 13, style: .continuous)
                    .fill(MelloTVTheme.sunken.opacity(0.75))
                    .overlay {
                        RoundedRectangle(cornerRadius: 13, style: .continuous)
                            .strokeBorder(MelloTVTheme.subtleBorder, lineWidth: 1)
                    }
                if checking && online {
                    ProgressView().controlSize(.small)
                } else {
                    Image(systemName: verified ? "checkmark" : failed ? "exclamationmark" : "minus")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(tone)
                }
            }
            .frame(width: 38, height: 38)
            .accessibilityHidden(true)

            VStack(alignment: .leading, spacing: 6) {
                Text(provider.name)
                    .font(.system(size: 15, weight: .semibold))
                    .tracking(-0.3)
                    .foregroundStyle(MelloTVTheme.text)
                Text(detail)
                    .font(.system(size: 11))
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 16)
            VStack(alignment: .trailing, spacing: 7) {
                Text(status)
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(tone)
                    .padding(.horizontal, 11)
                    .padding(.vertical, 6)
                    .background(tone.opacity(0.09), in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                if online && !checking, let report {
                    Text(Date(timeIntervalSince1970: report.checkedAt), style: .relative)
                        .font(.system(size: 10))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .help("Time since the independent check completed")
                }
            }
        }
        .padding(.horizontal, 24)
        .padding(.vertical, compact ? 12 : 16)
        .accessibilityElement(children: .combine)
    }
}

private struct ProviderHealthReport: Decodable {
    let id: String
    let version: String
    let working: Bool
    let checkedAt: Double
    let detail: String
}

private struct ProviderHealthResponse: Decodable {
    let providers: [ProviderHealthReport]
}

@MainActor
private final class ProviderHealthStore: ObservableObject {
    @Published private(set) var reports: [String: ProviderHealthReport] = [:]
    @Published private(set) var isChecking = false
    @Published private(set) var error: String?
    private var request: Task<Void, Never>?
    private var generation: UInt64 = 0

    func report(for provider: ServerSnapshot.ProviderStatus) -> ProviderHealthReport? {
        guard let report = reports[provider.id], report.version == provider.version else { return nil }
        return report
    }

    func check(providers: [ServerSnapshot.ProviderStatus], port: Int, force: Bool = false) {
        guard !isChecking, !providers.isEmpty else { return }
        if !force && providers.allSatisfy({ provider in
            guard let report = report(for: provider) else { return false }
            return Date().timeIntervalSince1970 - report.checkedAt < 60
        }) { return }
        let current = generation
        reports = [:]
        error = nil
        isChecking = true
        request = Task { [weak self] in
            guard let self else { return }
            defer { if current == self.generation { self.isChecking = false } }
            do {
                let url = AppLinks.clientURL(port: port).appendingPathComponent("api/v1/provider-health")
                var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 90)
                request.httpMethod = "POST"
                let (data, response) = try await URLSession.shared.data(for: request)
                guard !Task.isCancelled, current == self.generation else { return }
                let status = (response as? HTTPURLResponse)?.statusCode
                if status == 404 || status == 405 {
                    self.error = "This server doesn't support independent checks yet. Update the server and try again."
                    return
                }
                guard status == 200 else { throw URLError(.badServerResponse) }
                let decoder = JSONDecoder()
                decoder.keyDecodingStrategy = .convertFromSnakeCase
                let result = try decoder.decode(ProviderHealthResponse.self, from: data)
                self.reports = result.providers.reduce(into: [:]) { $0[$1.id] = $1 }
                if !providers.allSatisfy({ self.report(for: $0) != nil }) {
                    self.reports = [:]
                    self.error = "Provider information changed. Run the check again."
                }
            } catch {
                guard !Task.isCancelled, current == self.generation else { return }
                self.error = "Could not complete provider checks. Check the server connection and try again."
            }
        }
    }

    func cancel() {
        generation &+= 1
        request?.cancel()
        request = nil
        isChecking = false
    }

    func reset() {
        cancel()
        reports = [:]
        error = nil
    }
}
