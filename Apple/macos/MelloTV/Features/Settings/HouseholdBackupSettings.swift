import AppKit
import SwiftUI

struct HouseholdBackupSettings: View {
    @ObservedObject var coordinator: HouseholdBackupCoordinator
    @State private var passphrase = ""
    @State private var confirmation = ""
    @State private var includeCredential = false
    @State private var restoreCredential = false
    @State private var restoreURL: URL?
    @State private var confirmRestore = false
    @State private var localMessage: String?

    private var matching: Bool { passphrase.count >= 12 && passphrase == confirmation && passphrase.utf8.count <= 4096 }
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Household backup").font(.headline)
            Text("Back up profiles, history, preferences, and cached catalog data in an encrypted file. Choose a strong, memorable passphrase. You’ll need it to restore on another Mac.")
                .font(.caption).foregroundColor(.secondary).fixedSize(horizontal: false, vertical: true)
            HStack {
                SecureField("Backup passphrase", text: $passphrase)
                SecureField("Confirm passphrase", text: $confirmation)
            }
            Text("Use at least 12 characters. This passphrase protects the backup file; it is not an account or sign-in.")
                .font(.caption).foregroundColor(.secondary).fixedSize(horizontal: false, vertical: true)
            Toggle("Include my personal TMDB credential", isOn: $includeCredential)
                .help("The credential is included only inside the encrypted backup.")
            HStack {
                Button("Save encrypted backup…") { chooseBackupDestination() }
                    .disabled(!matching || coordinator.busy)
                Button("Restore backup…") { chooseRestoreSource() }
                    .disabled(passphrase.isEmpty || coordinator.busy)
            }
            Toggle("Restore personal TMDB credential if included", isOn: $restoreCredential)
                .font(.caption)
            Divider()
            HStack {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Automatic daily backups").font(.subheadline)
                    Text("Stores the passphrase in this Mac’s Keychain and keeps seven backups in the server’s Backups folder. Remember it for another Mac.")
                        .font(.caption).foregroundColor(.secondary).fixedSize(horizontal: false, vertical: true)
                }
                Spacer()
                if coordinator.automaticEnabled {
                    Button("Turn off") { disableAutomatic() }.disabled(coordinator.busy)
                } else {
                    Button("Turn on") { enableAutomatic() }.disabled(!matching || coordinator.busy)
                }
            }
            if coordinator.busy { ProgressView().controlSize(.small) }
            if let message = localMessage ?? coordinator.message {
                Text(message).font(.caption).foregroundColor(.secondary).fixedSize(horizontal: false, vertical: true)
            }
        }
        .confirmationDialog("Restore this household backup?", isPresented: $confirmRestore, titleVisibility: .visible) {
            Button("Restore backup", role: .destructive) {
                guard let restoreURL else { return }
                let password = passphrase
                let include = restoreCredential
                Task { await coordinator.restore(from: restoreURL, passphrase: password, restoreCredential: include) }
            }
            Button("Cancel", role: .cancel) { restoreURL = nil }
        } message: {
            Text("This replaces the server’s profiles, history, preferences, and catalog. The server will stop during restore, and a private rollback copy will be kept.")
        }
    }
    private func chooseBackupDestination() {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "Mello TV Backup.mellobackup"
        panel.canCreateDirectories = true
        panel.begin { response in
            guard response == .OK, let url = panel.url else { return }
            let password = passphrase
            let include = includeCredential
            localMessage = nil
            Task { await coordinator.create(to: url, passphrase: password, includeCredential: include) }
        }
    }
    private func chooseRestoreSource() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = false
        panel.canChooseFiles = true
        panel.allowsMultipleSelection = false
        panel.begin { response in
            guard response == .OK, let url = panel.url else { return }
            localMessage = nil
            restoreURL = url
            confirmRestore = true
        }
    }
    private func enableAutomatic() {
        do { try coordinator.enableAutomatic(passphrase: passphrase, includeCredential: includeCredential); localMessage = nil }
        catch { localMessage = error.localizedDescription }
    }
    private func disableAutomatic() {
        do { try coordinator.disableAutomatic(); localMessage = nil }
        catch { localMessage = error.localizedDescription }
    }
}
