import SwiftUI

struct ProfileEditorView: View {
    @Bindable var profileStore: ProfileStore
    @Environment(\.dismiss) private var dismiss

    @State private var name: String = ""
    @State private var selectedColorHex: String = ProfileColor.options[0]
    @State private var isSaving = false
    @State private var errorMessage: String?
    @State private var showDeleteConfirmation = false
    @State private var showPreferenceSetup = false

    private var isEditing: Bool {
        profileStore.editingProfile != nil
    }

    private var canDelete: Bool {
        isEditing && profileStore.profiles.count > 1
    }

    var body: some View {
        NavigationStack {
            ZStack {
                FamilyStreamTheme.background
                    .ignoresSafeArea()

                ScrollView {
                    VStack(spacing: 32) {
                        // Avatar Preview
                        VStack(spacing: 12) {
                            ProfileAvatarView(name: name, colorHex: selectedColorHex, size: 96)
                                .overlay(
                                    Circle()
                                        .stroke(FamilyStreamTheme.subtleBorder, lineWidth: 2)
                                )

                            Text(isEditing ? "Edit Profile" : "New Profile")
                                .font(.title3.weight(.bold))
                                .foregroundStyle(FamilyStreamTheme.text)
                        }
                        .padding(.top, 16)

                        // Name input
                        VStack(alignment: .leading, spacing: 8) {
                            Text("Name")
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(FamilyStreamTheme.muted)

                            TextField("Profile Name", text: $name)
                                .font(.body)
                                .foregroundStyle(FamilyStreamTheme.text)
                                .padding(12)
                                .background(FamilyStreamTheme.cardBackground, in: RoundedRectangle(cornerRadius: 8))
                                .overlay(
                                    RoundedRectangle(cornerRadius: 8)
                                        .stroke(FamilyStreamTheme.border, lineWidth: 1)
                                )
                                .autocorrectionDisabled()
                        }
                        .padding(.horizontal, 24)

                        // Color selection
                        VStack(alignment: .leading, spacing: 12) {
                            Text("Avatar Color")
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(FamilyStreamTheme.muted)

                            HStack(spacing: 16) {
                                ForEach(ProfileColor.options, id: \.self) { colorHex in
                                    Button {
                                        selectedColorHex = colorHex
                                    } label: {
                                        Circle()
                                            .fill(ProfileColor.color(for: colorHex))
                                            .frame(width: 44, height: 44)
                                            .overlay(
                                                Circle()
                                                    .stroke(
                                                        selectedColorHex.lowercased() == colorHex.lowercased()
                                                            ? FamilyStreamTheme.text
                                                            : Color.clear,
                                                        lineWidth: 3
                                                    )
                                                    .padding(-4)
                                            )
                                            .touchTarget()
                                    }
                                    .buttonStyle(.plain)
                                    .accessibilityLabel(ProfileColor.name(for: colorHex))
                                 }
                            }
                            .frame(maxWidth: .infinity, alignment: .center)
                        }
                        .padding(.horizontal, 24)

                        // Edit preferences button if editing
                        if let editingProfile = profileStore.editingProfile {
                            Button {
                                showPreferenceSetup = true
                            } label: {
                                HStack {
                                    PhosphorIcon("sliders-horizontal", size: 16)
                                    Text("Adjust Viewing Preferences")
                                    Spacer()
                                    PhosphorIcon("caret-right", size: 12, weight: .bold)
                                        .foregroundStyle(FamilyStreamTheme.muted)
                                }
                                .font(.subheadline.weight(.medium))
                                .foregroundStyle(FamilyStreamTheme.text)
                                .padding(14)
                                .background(FamilyStreamTheme.cardBackground, in: RoundedRectangle(cornerRadius: 8))
                                .overlay(
                                    RoundedRectangle(cornerRadius: 8)
                                        .stroke(FamilyStreamTheme.border, lineWidth: 1)
                                )
                            }
                            .buttonStyle(.plain)
                            .padding(.horizontal, 24)
                        }

                        if let errorMessage {
                            Text(errorMessage)
                                .font(.caption)
                                .foregroundStyle(.red)
                                .multilineTextAlignment(.center)
                                .padding(.horizontal, 24)
                        }

                        // Save button
                        Button {
                            save()
                        } label: {
                            if isSaving {
                                ProgressView()
                                    .tint(.white)
                            } else {
                                Text(isEditing ? "Save" : "Create Profile")
                                    .font(.headline)
                                    .foregroundStyle(name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? FamilyStreamTheme.muted : .white)
                                    .frame(maxWidth: .infinity)
                                    .padding(.vertical, 14)
                                    .background(
                                        name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                                            ? FamilyStreamTheme.border.opacity(0.5)
                                            : FamilyStreamTheme.accent,
                                        in: RoundedRectangle(cornerRadius: 8)
                                    )
                            }
                        }
                        .disabled(name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || isSaving)
                        .buttonStyle(.plain)
                        .padding(.horizontal, 24)

                        // Delete button (if allowed)
                        if canDelete, let editing = profileStore.editingProfile {
                            Button {
                                showDeleteConfirmation = true
                            } label: {
                                Text("Delete Profile")
                                    .font(.subheadline.weight(.medium))
                                    .foregroundStyle(.red)
                                    .padding(.vertical, 10)
                            }
                            .buttonStyle(.plain)
                            .confirmationDialog(
                                "Delete \(editing.name)?",
                                isPresented: $showDeleteConfirmation,
                                titleVisibility: .visible
                            ) {
                                Button("Delete Profile", role: .destructive) {
                                    delete(editing)
                                }
                                Button("Cancel", role: .cancel) {}
                            } message: {
                                Text("All viewing history, progress, and recommendations for \(editing.name) will be permanently deleted.")
                            }
                        }
                    }
                    .padding(.vertical, 16)
                }
            }
            .navigationTitle(isEditing ? "Edit Profile" : "New Profile")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                    .foregroundStyle(FamilyStreamTheme.accent)
                }
            }
            .onAppear {
                if let editing = profileStore.editingProfile {
                    name = editing.name
                    selectedColorHex = editing.color
                } else {
                    name = ""
                    // Pick the next unused color
                    let usedColors = Set(profileStore.profiles.map { $0.color.lowercased() })
                    if let unused = ProfileColor.options.first(where: { !usedColors.contains($0.lowercased()) }) {
                        selectedColorHex = unused
                    } else {
                        selectedColorHex = ProfileColor.options[0]
                    }
                }
            }
            .fullScreenCover(isPresented: $showPreferenceSetup) {
                if let editing = profileStore.editingProfile {
                    OnboardingView(
                        profileStore: profileStore,
                        profile: editing,
                        onCompleted: {
                            showPreferenceSetup = false
                            dismiss()
                        }
                    )
                }
            }
        }
    }

    private func save() {
        guard !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        isSaving = true
        errorMessage = nil

        Task {
            do {
                try await profileStore.saveProfile(
                    name: name,
                    color: selectedColorHex,
                    existingID: profileStore.editingProfile?.id
                )
                isSaving = false
                dismiss()
            } catch {
                isSaving = false
                errorMessage = error.localizedDescription
            }
        }
    }

    private func delete(_ profile: Profile) {
        Task {
            do {
                try await profileStore.deleteProfile(profile)
                dismiss()
            } catch {
                errorMessage = error.localizedDescription
            }
        }
    }
}
