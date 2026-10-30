import SwiftUI
import MelloKit

@MainActor
struct UserEditorView: View {
    let profile: Profile?
    @ObservedObject var store: UsersStore
    let port: Int
    let onComplete: (Profile?) -> Void
    @State private var name: String
    @State private var color: String
    @State private var error: String?
    @State private var confirmingDelete = false
    @Environment(\.dismiss) private var dismiss
    @FocusState private var nameFocused: Bool

    init(profile: Profile?, store: UsersStore, port: Int, onComplete: @escaping (Profile?) -> Void) {
        self.profile = profile
        self.store = store
        self.port = port
        self.onComplete = onComplete
        _name = State(initialValue: profile?.name ?? "")
        _color = State(initialValue: profile?.color ?? ProfileColor.options[0])
    }

    private var cleanName: String { name.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var validName: Bool { !cleanName.isEmpty && cleanName.utf8.count <= 80 }
    private var preview: Profile {
        Profile(id: profile?.id ?? "preview", name: cleanName.isEmpty ? "?" : cleanName,
                color: color, preferences: profile?.preferences ?? .empty)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            HStack(spacing: 16) {
                UserAvatar(profile: preview, size: 58)
                VStack(alignment: .leading, spacing: 6) {
                    Text(profile == nil ? "Add a user" : "Edit profile")
                        .font(.system(size: 24, weight: .semibold)).tracking(-0.6)
                        .foregroundStyle(MelloTVTheme.text)
                    Text(profile == nil ? "Their own space to watch." : "Update their name and profile color.")
                        .font(.system(size: 12)).foregroundStyle(MelloTVTheme.textSecondary)
                }
            }
            VStack(alignment: .leading, spacing: 9) {
                Text("Name").font(.system(size: 12, weight: .medium))
                    .foregroundStyle(MelloTVTheme.text)
                TextField("Profile name", text: $name)
                    .textFieldStyle(.roundedBorder).focused($nameFocused).disabled(store.isSaving)
                    .onSubmit { if validName && !store.isSaving { save() } }
                if cleanName.utf8.count > 80 {
                    Text("Use a shorter name (up to 80 UTF-8 bytes).")
                        .font(.system(size: 11)).foregroundStyle(.orange)
                }
            }
            VStack(alignment: .leading, spacing: 12) {
                Text("Profile color").font(.system(size: 12, weight: .medium))
                    .foregroundStyle(MelloTVTheme.text)
                HStack(spacing: 12) {
                    ForEach(ProfileColor.options, id: \.self) { option in
                        Button { color = option } label: {
                            ZStack {
                                RoundedRectangle(cornerRadius: 12).fill(ProfileColor.color(for: option))
                                if color.caseInsensitiveCompare(option) == .orderedSame {
                                    Image(systemName: "checkmark").font(.system(size: 12, weight: .bold)).foregroundStyle(.white)
                                }
                            }
                            .frame(width: 40, height: 40)
                            .overlay { RoundedRectangle(cornerRadius: 12).strokeBorder(.white.opacity(0.18), lineWidth: 1) }
                        }
                        .buttonStyle(OverviewPressStyle()).disabled(store.isSaving)
                        .accessibilityLabel(ProfileColor.name(for: option))
                        .accessibilityAddTraits(color.caseInsensitiveCompare(option) == .orderedSame ? .isSelected : [])
                    }
                }
            }
            if profile == nil {
                Text("They can choose their tastes when they select this profile on a viewing device.")
                    .font(.system(size: 11)).lineSpacing(3).foregroundStyle(MelloTVTheme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let error {
                Text(error).font(.system(size: 12)).foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack(spacing: 12) {
                if profile != nil {
                    Button("Delete user", role: .destructive) { confirmingDelete = true }
                        .disabled(store.isSaving)
                }
                Spacer()
                if store.isSaving { ProgressView().controlSize(.small) }
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction).disabled(store.isSaving)
                Button(profile == nil ? "Add user" : "Save") { save() }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(.borderedProminent).tint(MelloTVTheme.accent)
                    .disabled(!validName || store.isSaving)
            }
        }
        .padding(30).frame(width: 430)
        .background(MelloTVTheme.background)
        .interactiveDismissDisabled(store.isSaving)
        .onAppear { nameFocused = true }
        .confirmationDialog("Delete \(profile?.name ?? "this user")?", isPresented: $confirmingDelete, titleVisibility: .visible) {
            Button("Delete user and viewing data", role: .destructive) { delete() }
            Button("Cancel", role: .cancel) { }
        } message: {
            Text("This permanently removes their profile, saved titles, watch history, and preferences from this server. This cannot be undone.")
        }
    }

    private func save() {
        error = nil
        Task {
            do {
                let saved = try await store.save(profile: profile, name: cleanName, color: color, port: port)
                onComplete(saved)
                dismiss()
            } catch { self.error = error.localizedDescription }
        }
    }

    private func delete() {
        guard let profile else { return }
        error = nil
        Task {
            do {
                try await store.delete(profile, port: port)
                onComplete(nil)
                dismiss()
            } catch { self.error = error.localizedDescription }
        }
    }
}
