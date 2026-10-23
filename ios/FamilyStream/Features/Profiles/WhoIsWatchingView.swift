import SwiftUI

struct WhoIsWatchingView: View {
    @Bindable var profileStore: ProfileStore

    private let columns = [
        GridItem(.adaptive(minimum: 120, maximum: 160), spacing: 28)
    ]

    var body: some View {
        ZStack {
            FamilyStreamTheme.background
                .ignoresSafeArea()

            if profileStore.isLoading && profileStore.profiles.isEmpty {
                ProgressView()
                    .tint(FamilyStreamTheme.accent)
            } else if profileStore.profiles.isEmpty {
                emptyState
            } else {
                content
            }
        }
        .sheet(isPresented: $profileStore.showProfileEditor) {
            ProfileEditorView(profileStore: profileStore)
        }
    }

    private var content: some View {
        VStack(spacing: 36) {
            Spacer()

            VStack(spacing: 10) {
                Text(profileStore.isManagingProfiles ? "Manage Profiles" : "Who's Watching?")
                    .font(.system(size: 34, weight: .bold, design: .rounded))
                    .foregroundStyle(FamilyStreamTheme.text)

                if profileStore.isManagingProfiles {
                    Text("Select a profile to edit its name, color, or preferences.")
                        .font(.subheadline)
                        .foregroundStyle(FamilyStreamTheme.muted)
                }
            }

            LazyVGrid(columns: columns, spacing: 32) {
                ForEach(profileStore.profiles) { profile in
                    profileButton(for: profile)
                }

                if profileStore.profiles.count < 6 {
                    addProfileButton
                }
            }
            .padding(.horizontal, 32)
            .frame(maxWidth: 600)

            Spacer()

            Button {
                withAnimation(.spring(response: 0.3, dampingFraction: 0.8)) {
                    profileStore.isManagingProfiles.toggle()
                }
            } label: {
                Text(profileStore.isManagingProfiles ? "Done" : "Manage Profiles")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(FamilyStreamTheme.text)
                    .padding(.horizontal, 28)
                    .padding(.vertical, 12)
                    .background(FamilyStreamTheme.cardBackground, in: RoundedRectangle(cornerRadius: 8))
                    .overlay(
                        RoundedRectangle(cornerRadius: 8)
                            .stroke(FamilyStreamTheme.border, lineWidth: 1)
                    )
            }
            .buttonStyle(CardScaleButtonStyle(scale: 0.96))
            .padding(.bottom, 36)
        }
    }

    private func profileButton(for profile: Profile) -> some View {
        Button {
            if profileStore.isManagingProfiles {
                profileStore.openEditProfile(profile)
            } else {
                profileStore.selectProfile(profile)
            }
        } label: {
            VStack(spacing: 12) {
                ZStack {
                    ProfileAvatarView(name: profile.name, colorHex: profile.color, size: 104)

                    if profileStore.isManagingProfiles {
                        RoundedRectangle(cornerRadius: 18, style: .continuous)
                            .fill(.black.opacity(0.6))
                            .frame(width: 104, height: 104)

                        PhosphorIcon("pencil-simple", size: 24, weight: .bold)
                            .foregroundStyle(.white)
                    }
                }

                Text(profile.name)
                    .font(.callout.weight(.medium))
                    .foregroundStyle(FamilyStreamTheme.text)
                    .lineLimit(1)
            }
        }
        .buttonStyle(CardScaleButtonStyle(scale: 0.94))
    }

    private var addProfileButton: some View {
        Button {
            profileStore.openCreateProfile()
        } label: {
            VStack(spacing: 12) {
                ZStack {
                    RoundedRectangle(cornerRadius: 18, style: .continuous)
                        .fill(FamilyStreamTheme.cardBackground)
                        .frame(width: 104, height: 104)
                        .overlay(
                            RoundedRectangle(cornerRadius: 18, style: .continuous)
                                .stroke(FamilyStreamTheme.border, lineWidth: 1.5)
                        )

                    PhosphorIcon("plus", size: 28, weight: .bold)
                        .foregroundStyle(FamilyStreamTheme.muted)
                }

                Text("Add Profile")
                    .font(.callout.weight(.medium))
                    .foregroundStyle(FamilyStreamTheme.muted)
            }
        }
        .buttonStyle(CardScaleButtonStyle(scale: 0.94))
    }

    private var emptyState: some View {
        VStack(spacing: 20) {
            PhosphorIcon("users", size: 60, weight: .fill)
                .foregroundStyle(FamilyStreamTheme.muted)

            Text("No Profiles Found")
                .font(.title2.weight(.bold))
                .foregroundStyle(FamilyStreamTheme.text)

            Text("Create a profile to personalize your family streaming experience.")
                .font(.subheadline)
                .foregroundStyle(FamilyStreamTheme.muted)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)

            Button {
                profileStore.openCreateProfile()
            } label: {
                Text("Add Profile")
                    .font(.headline)
                    .foregroundStyle(.white)
                    .padding(.horizontal, 32)
                    .padding(.vertical, 14)
                    .background(FamilyStreamTheme.accent, in: RoundedRectangle(cornerRadius: 8))
            }
            .buttonStyle(CardScaleButtonStyle(scale: 0.96))
        }
        .padding()
    }
}
