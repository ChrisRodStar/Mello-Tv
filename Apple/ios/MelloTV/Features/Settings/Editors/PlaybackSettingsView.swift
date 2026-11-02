import MelloKit
import SwiftUI

struct PlaybackSettingsView: View {
    let profileID: String
    @Environment(\.dismiss) private var dismiss
    @State private var settings = ProfilePlaybackSettings()
    @State private var originalSettings: ProfilePlaybackSettings?
    @State private var loaded = false
    @State private var saving = false
    @State private var error: String?
    private let api = MelloTVAPI(baseURL: ServerAddress.baseURL)

    var body: some View {
        NavigationStack {
            Form {
                Section("Languages") {
                    TextField("Audio language code (e.g. en)", text: $settings.audioLanguage)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                    Toggle("Subtitles by default", isOn: $settings.subtitlesEnabled)
                    TextField("Subtitle language code (e.g. en)", text: $settings.subtitleLanguage)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                        .disabled(!settings.subtitlesEnabled)
                }
                Section("Playback") {
                    Toggle("Autoplay next episode", isOn: $settings.autoplayNext)
                    Picker("Default quality", selection: $settings.maxHeight) {
                        Text("Auto").tag(0)
                        Text("1080p").tag(1080)
                        Text("720p").tag(720)
                        Text("480p").tag(480)
                        Text("360p").tag(360)
                    }
                }
                if let error { Text(error).foregroundStyle(.red) }
            }
            .disabled(!loaded || saving)
            .navigationTitle("Playback settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { Task { await save() } }.disabled(!loaded || saving)
                }
            }
            .task {
                do {
                    let saved = try await api.playbackSettings(profileID: profileID)
                    settings = saved
                    originalSettings = saved
                    loaded = true
                } catch { self.error = "Couldn’t load playback settings. Reopen to try again." }
            }
        }
    }
    private func save() async {
        saving = true
        defer { saving = false }
        do {
            if settings != originalSettings {
                guard try await api.playbackSettings(profileID: profileID) == originalSettings
                else {
                    self.error =
                        "Playback settings changed on another device. Reopen to review them."
                    return
                }
                try await api.savePlaybackSettings(settings, profileID: profileID)
            }
            dismiss()
        } catch { self.error = "Couldn’t save settings. Check the language codes and try again." }
    }
}
