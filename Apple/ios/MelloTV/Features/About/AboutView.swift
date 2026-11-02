import MelloKit
import SwiftUI

struct AboutView: View {
    @State private var showsArtworkCredits = false

    var body: some View {
        ScrollView {
            VStack(spacing: 28) {
                AboutAppHeader()
                aboutContent
            }
            .padding(.horizontal, 24)
            .padding(.top, 20)
            .padding(.bottom, 32)
            .frame(maxWidth: 640)
            .frame(maxWidth: .infinity)
        }
        .scrollBounceBehavior(.basedOnSize)
        .background { CatalogPageSurface().ignoresSafeArea() }
        .navigationBarTitleDisplayMode(.inline)
        .toolbar(.visible, for: .navigationBar)
        .sheet(isPresented: $showsArtworkCredits) {
            OnboardingArtworkCredits()
        }
    }

    private var aboutContent: some View {
        VStack(alignment: .leading, spacing: 20) {
            VStack(spacing: 0) {
                Link(destination: AppLinks.docsURL) {
                    SettingsRow(title: "Help")
                }
                SettingsRowDivider()
                Button {
                    showsArtworkCredits = true
                } label: {
                    SettingsRow(title: "Artwork credits")
                }
                SettingsRowDivider()
                Link(destination: AppLinks.githubURL) {
                    SettingsRow(title: "Source code")
                }
            }
            .appleNeumorphic(cornerRadius: 20, elevation: .subtle)
            .buttonStyle(.plain)

            Link(destination: URL(string: "https://www.themoviedb.org/")!) {
                HStack(alignment: .center, spacing: 12) {
                    Image("TMDBLogo")
                        .resizable()
                        .scaledToFit()
                        .frame(width: 40, height: 28)
                        .accessibilityHidden(true)
                    Text("This product uses the TMDB API but is not endorsed or certified by TMDB.")
                        .font(.caption)
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(minHeight: 44)
            }
            .padding(18)
            .appleNeumorphicRecessed(cornerRadius: 16)
        }
    }
}
