import SwiftUI
import MelloKit

private struct UserEditorTarget: Identifiable {
    let profile: Profile?
    var id: String { profile?.id ?? "new" }
}

@MainActor
struct UsersView: View {
    let online: Bool
    let starting: Bool
    let port: Int
    @StateObject private var store = UsersStore()
    @State private var selectedID: String?
    @State private var editor: UserEditorTarget?
    @State private var query = ""

    private var selected: Profile? { store.profiles.first { $0.id == selectedID } }
    private var visibleProfiles: [Profile] {
        store.profiles.filter { query.isEmpty || $0.name.localizedCaseInsensitiveContains(query) }
    }

    var body: some View {
        GeometryReader { viewport in
            VStack(alignment: .leading, spacing: 25) {
                header
                if let error = store.listError {
                    HStack {
                        Text(error).font(.system(size: 12)).foregroundStyle(MelloTVTheme.textSecondary)
                        Spacer()
                        Button("Retry") { Task { await store.refresh(port: port) } }
                            .disabled(!online || store.isLoading)
                    }
                }
                if !online || !store.hasLoaded || store.profiles.isEmpty {
                    emptyState
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .background(UsersSurface())
                } else {
                    HStack(alignment: .top, spacing: 0) {
                        profileList.frame(width: min(260, viewport.size.width * 0.31))
                        Rectangle().fill(MelloTVTheme.border.opacity(0.55)).frame(width: 1)
                        ScrollView {
                            if let profile = selected {
                                UserDetailView(profile: profile,
                                               details: store.details?.profileID == profile.id ? store.details : nil,
                                               loading: store.isLoadingDetails,
                                               edit: { editor = UserEditorTarget(profile: profile) })
                                    .padding(26)
                            } else {
                                Text("Choose a profile to see their activity.")
                                    .font(.system(size: 13)).foregroundStyle(MelloTVTheme.textSecondary)
                                    .frame(maxWidth: .infinity, minHeight: 240)
                            }
                        }
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                    }
                    .background(UsersSurface())
                }
            }
            .padding(viewport.size.height < 620 ? 26 : 34)
        }
        .task(id: online) {
            store.setOnline(online)
            guard online else { return }
            while !Task.isCancelled {
                await store.refresh(port: port)
                guard !Task.isCancelled else { return }
                if !store.profiles.contains(where: { $0.id == selectedID }) { selectedID = store.profiles.first?.id }
                do { try await Task.sleep(nanoseconds: 15_000_000_000) }
                catch { return }
            }
        }
        .task(id: "\(online)-\(selectedID ?? "none")") {
            guard online else { return }
            while !Task.isCancelled {
                await store.loadDetails(profileID: selectedID, port: port)
                do { try await Task.sleep(nanoseconds: 30_000_000_000) }
                catch { return }
            }
        }
        .onChange(of: online) { value in
            store.setOnline(value)
            if !value { selectedID = nil; editor = nil }
        }
        .sheet(item: $editor) { target in
            UserEditorView(profile: target.profile, store: store, port: port) { saved in
                selectedID = saved?.id ?? store.profiles.first?.id
            }
        }
    }

    private var header: some View {
        HStack(spacing: 20) {
            VStack(alignment: .leading, spacing: 9) {
                Text(starting ? "Your server is starting." : !online ? "Your server is offline." : store.hasLoaded
                     ? "\(store.profiles.count) viewing \(store.profiles.count == 1 ? "profile" : "profiles")." : "Loading your profiles.")
                    .font(.system(size: 30, weight: .semibold)).tracking(-1)
                    .foregroundStyle(MelloTVTheme.text).accessibilityAddTraits(.isHeader)
                Text("Individual watch history, saved titles, and tastes.")
                    .font(.system(size: 12)).foregroundStyle(MelloTVTheme.textSecondary)
            }
            Spacer(minLength: 0)
            Button { editor = UserEditorTarget(profile: nil) } label: {
                Label("Add user", systemImage: "plus")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(online ? Color.white : MelloTVTheme.textSecondary)
                    .padding(.horizontal, 17).frame(height: 40)
                    .background(online ? MelloTVTheme.accent : MelloTVTheme.sunken,
                                in: RoundedRectangle(cornerRadius: 13))
            }
            .buttonStyle(OverviewPressStyle()).disabled(!online || store.isSaving)
        }
        .padding(.horizontal, 4)
    }

    private var profileList: some View {
        VStack(spacing: 12) {
            if store.profiles.count > 8 {
                TextField("Find a user", text: $query).textFieldStyle(.roundedBorder)
                    .padding(.horizontal, 14).padding(.top, 6)
            }
            ScrollView {
                VStack(spacing: 5) {
                    ForEach(visibleProfiles) { profile in
                        Button { selectedID = profile.id } label: {
                            HStack(spacing: 11) {
                                UserAvatar(profile: profile, size: 38)
                                VStack(alignment: .leading, spacing: 5) {
                                    Text(profile.name).font(.system(size: 13, weight: .semibold))
                                        .foregroundStyle(MelloTVTheme.text).lineLimit(1)
                                    Text(profile.preferences.completed ? "Personalized" : "Finish setup on a device")
                                        .font(.system(size: 10)).foregroundStyle(MelloTVTheme.textSecondary)
                                        .lineLimit(2)
                                }
                                Spacer(minLength: 0)
                            }
                            .padding(11).frame(maxWidth: .infinity, alignment: .leading)
                            .background(selectedID == profile.id ? MelloTVTheme.sunken.opacity(0.8) : .clear,
                                        in: RoundedRectangle(cornerRadius: 16))
                            .overlay {
                                RoundedRectangle(cornerRadius: 16)
                                    .strokeBorder(selectedID == profile.id ? MelloTVTheme.subtleBorder : .clear, lineWidth: 1)
                            }
                            .contentShape(RoundedRectangle(cornerRadius: 16))
                        }
                        .buttonStyle(OverviewPressStyle())
                        .accessibilityAddTraits(selectedID == profile.id ? .isSelected : [])
                    }
                    if visibleProfiles.isEmpty {
                        Text("No matching users").font(.system(size: 12))
                            .foregroundStyle(MelloTVTheme.textSecondary).padding(20)
                    }
                }
                .padding(.horizontal, 12)
            }
        }
        .padding(.vertical, 14)
    }

    private var emptyState: some View {
        VStack(spacing: 15) {
            if online && !store.hasLoaded && store.listError == nil {
                ProgressView().controlSize(.small)
            } else {
                Image(systemName: "person.2").font(.system(size: 32, weight: .light))
                    .foregroundStyle(MelloTVTheme.muted)
            }
            Text(starting ? "Profiles appear when the server is ready." : !online
                 ? "Start your server from Overview to see your users." : store.listError != nil
                 ? "Profiles are unavailable right now." : store.hasLoaded
                 ? "Make room for your first viewer." : "Loading profiles…")
                .font(.system(size: 14, weight: .medium)).foregroundStyle(MelloTVTheme.text)
            if online && store.hasLoaded && store.profiles.isEmpty {
                Text("Add a user here, then choose their tastes in Mello TV on a viewing device.")
                    .font(.system(size: 12)).foregroundStyle(MelloTVTheme.textSecondary)
            }
        }
        .padding(30)
    }
}

private struct UserDetailView: View {
    let profile: Profile
    let details: UserDetails?
    let loading: Bool
    let edit: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 27) {
            HStack(spacing: 14) {
                UserAvatar(profile: profile, size: 56)
                VStack(alignment: .leading, spacing: 5) {
                    Text(profile.name).font(.system(size: 23, weight: .semibold)).tracking(-0.5)
                        .foregroundStyle(MelloTVTheme.text).lineLimit(2)
                    Text(profile.preferences.completed ? "Viewing profile" : "Taste setup not finished")
                        .font(.system(size: 11)).foregroundStyle(MelloTVTheme.textSecondary)
                }
                Spacer(minLength: 8)
                Button(action: edit) {
                    Label("Edit", systemImage: "pencil")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(MelloTVTheme.text)
                        .padding(.horizontal, 13).frame(height: 32)
                        .background(MelloTVTheme.sunken, in: RoundedRectangle(cornerRadius: 10))
                }
                .buttonStyle(OverviewPressStyle())
            }
            HStack(alignment: .top, spacing: 24) {
                metric("Saved titles", value: details?.savedCount.map(String.init) ?? (loading ? "…" : "Unavailable"))
                Rectangle().fill(MelloTVTheme.border).frame(width: 1, height: 38)
                VStack(alignment: .leading, spacing: 7) {
                    Text("Last watched").font(.system(size: 10)).foregroundStyle(MelloTVTheme.textSecondary)
                    if let date = details?.history?.first?.date {
                        Text(date, style: .relative).font(.system(size: 13, weight: .semibold))
                            .foregroundStyle(MelloTVTheme.text).help(date.formatted())
                    } else {
                        Text(loading ? "…" : details?.history == nil ? "Unavailable" : "No history yet")
                            .font(.system(size: 13, weight: .semibold)).foregroundStyle(MelloTVTheme.text)
                    }
                }
            }
            .padding(17).frame(maxWidth: .infinity, alignment: .leading)
            .background(MelloTVTheme.sunken.opacity(0.65), in: RoundedRectangle(cornerRadius: 17))

            VStack(alignment: .leading, spacing: 11) {
                sectionLabel("Taste")
                Text(tasteDescription).font(.system(size: 12)).lineSpacing(4)
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            VStack(alignment: .leading, spacing: 17) {
                HStack {
                    sectionLabel("Recently watched")
                    Spacer()
                    if loading { ProgressView().controlSize(.mini) }
                }
                if let records = details?.history, !records.isEmpty {
                    ForEach(records, id: \.identity) { record in
                        watchRow(record)
                        if record.identity != records.last?.identity {
                            Rectangle().fill(MelloTVTheme.border.opacity(0.6)).frame(height: 1)
                        }
                    }
                } else {
                    Text(loading ? "Loading recent activity…" : details?.history == nil
                         ? "Recent activity couldn’t be loaded. It will refresh automatically."
                         : "Their watch history will appear here after they start watching.")
                        .font(.system(size: 12)).lineSpacing(3)
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var tasteDescription: String {
        guard profile.preferences.completed else {
            return "Choose this profile on a viewing device to finish setting up recommendations."
        }
        let format = profile.preferences.format == "movie" ? "Movies" : profile.preferences.format == "tv" ? "TV shows" : "Movies & TV shows"
        let genres = profile.preferences.genres.map { id in
            GenreOption.all.first(where: { $0.id == id })?.name ?? id.capitalized
        }
        return genres.isEmpty ? format : "\(format) · \(genres.joined(separator: ", "))"
    }

    private func sectionLabel(_ text: String) -> some View {
        Text(text).font(.system(size: 12, weight: .semibold))
            .foregroundStyle(MelloTVTheme.text).accessibilityAddTraits(.isHeader)
    }

    private func metric(_ title: String, value: String) -> some View {
        VStack(alignment: .leading, spacing: 7) {
            Text(title).font(.system(size: 10)).foregroundStyle(MelloTVTheme.textSecondary)
            Text(value).font(.system(size: 15, weight: .semibold)).monospacedDigit()
                .foregroundStyle(MelloTVTheme.text)
        }
    }

    private func watchRow(_ record: UserWatchRecord) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: record.type == "tv" ? "tv" : "film")
                .font(.system(size: 15, weight: .regular)).foregroundStyle(MelloTVTheme.muted)
                .frame(width: 24, height: 28)
            VStack(alignment: .leading, spacing: 7) {
                Text(details?.titles[record.titleKey] ?? "\(record.type == "tv" ? "TV show" : "Movie") · \(record.id)")
                    .font(.system(size: 12, weight: .medium)).foregroundStyle(MelloTVTheme.text)
                    .lineLimit(2)
                HStack(spacing: 7) {
                    if let season = record.season, let episode = record.episode {
                        Text("S\(season) E\(episode)")
                    }
                    Text(record.completed ? "Finished" : "\(Int(record.fraction * 100))% watched")
                    if let date = record.date { Text("·"); Text(date, style: .date) }
                }
                .font(.system(size: 10)).foregroundStyle(MelloTVTheme.textSecondary)
                if !record.completed {
                    GeometryReader { bar in
                        Capsule().fill(MelloTVTheme.border)
                        Capsule().fill(MelloTVTheme.accent.opacity(0.75))
                            .frame(width: bar.size.width * record.fraction)
                    }
                    .frame(height: 3).accessibilityHidden(true)
                }
            }
        }
    }
}

struct UserAvatar: View {
    let profile: Profile
    let size: CGFloat
    var body: some View {
        Text(String(profile.name.prefix(1)).uppercased())
            .font(.system(size: size * 0.38, weight: .semibold, design: .rounded))
            .foregroundStyle(.white.opacity(0.95))
            .frame(width: size, height: size)
            .background(LinearGradient(colors: [ProfileColor.color(for: profile.color), ProfileColor.color(for: profile.color).opacity(0.72)],
                                       startPoint: .topLeading, endPoint: .bottomTrailing),
                        in: RoundedRectangle(cornerRadius: size * 0.31))
            .overlay {
                RoundedRectangle(cornerRadius: size * 0.31)
                    .strokeBorder(.white.opacity(0.18), lineWidth: 1)
            }
            .accessibilityHidden(true)
    }
}

private struct UsersSurface: View {
    @Environment(\.colorScheme) private var colorScheme
    var body: some View {
        RoundedRectangle(cornerRadius: 28, style: .continuous)
            .fill(LinearGradient(stops: [
                .init(color: MelloTVTheme.surfaceHighlight, location: 0),
                .init(color: MelloTVTheme.card, location: 0.5),
                .init(color: MelloTVTheme.surfaceShade, location: 1)
            ], startPoint: .topLeading, endPoint: .bottomTrailing))
            .overlay {
                RoundedRectangle(cornerRadius: 28, style: .continuous)
                    .strokeBorder(LinearGradient(colors: [.white.opacity(colorScheme == .dark ? 0.10 : 0.85), MelloTVTheme.subtleBorder],
                                                 startPoint: .topLeading, endPoint: .bottomTrailing), lineWidth: 1)
            }
            .shadow(color: MelloTVTheme.ambientShadow.opacity(colorScheme == .dark ? 0.25 : 0.10), radius: 18, y: 12)
    }
}
