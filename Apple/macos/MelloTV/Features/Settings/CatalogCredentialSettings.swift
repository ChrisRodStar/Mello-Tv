import SwiftUI
import MelloKit

/// Owns only the transient input. The saved credential stays in Keychain.
struct CatalogCredentialSettings: View {
    let onRestart: () -> Void
    @State private var input = ""
    @State private var hasCredential = false
    @State private var isSaving = false
    @State private var message: String?
    @State private var isError = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Label("Catalog · TMDB", systemImage: "film.stack")
                .font(.headline)
            Text(hasCredential ? "Your personal TMDB credential is saved in Keychain." : "Catalog access is included. You can optionally use your own TMDB API key or API Read Access Token.")
                .font(.callout)
                .foregroundStyle(MelloTVTheme.textSecondary)
            SecureField(hasCredential ? "Replacement API key or Read Access Token" : "Optional API key or Read Access Token", text: $input)
                .textFieldStyle(.roundedBorder)
                .disabled(isSaving)
                .onSubmit { save() }
            HStack(spacing: 12) {
                Button(isSaving ? "Checking TMDB…" : "Save & Restart Server", action: save)
                    .disabled(isSaving || input.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                if hasCredential {
                    Button("Remove Override", role: .destructive, action: remove)
                        .disabled(isSaving)
                }
                Spacer()
                Link("Get a TMDB key", destination: URL(string: "https://www.themoviedb.org/settings/api")!)
            }
            .font(.callout)
            Text("Saving restarts the local server and interrupts active streams. Existing profiles and cached titles are kept.")
                .font(.caption)
                .foregroundStyle(MelloTVTheme.textSecondary)
            if let message {
                Text(message)
                    .font(.callout)
                    .foregroundStyle(isError ? Color.orange : MelloTVTheme.textSecondary)
                    .accessibilityLabel(message)
            }
        }
        .padding(24)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(MelloTVTheme.card, in: RoundedRectangle(cornerRadius: 20))
        .overlay { RoundedRectangle(cornerRadius: 20).strokeBorder(MelloTVTheme.border, lineWidth: 1) }
        .task { refresh() }
        .onDisappear { input = "" }
    }

    private func refresh() {
        do { hasCredential = try CatalogCredentialStore.read() != nil }
        catch { showError(error) }
    }

    private func save() {
        guard !isSaving, !input.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        isSaving = true
        message = nil
        let candidate = input
        input = ""
        Task { @MainActor in
            defer { isSaving = false }
            do {
                let validated = try await CatalogCredentialStore.validate(candidate)
                try CatalogCredentialStore.save(validated)
                hasCredential = true
                isError = false
                message = "Saved in Keychain. Restarting the server; return to profile setup to continue."
                onRestart()
            } catch { showError(error) }
        }
    }

    private func remove() {
        do {
            try CatalogCredentialStore.remove()
            hasCredential = false
            input = ""
            isError = false
            message = "Personal credential removed. Restarting with the default configuration."
            onRestart()
        } catch { showError(error) }
    }

    private func showError(_ error: Error) {
        isError = true
        message = error.localizedDescription
    }
}
