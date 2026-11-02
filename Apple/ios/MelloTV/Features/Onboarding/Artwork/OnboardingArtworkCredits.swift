import SwiftUI

struct OnboardingArtworkCredits: View {
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Text(
                        "Posters are shown as illustrations. The titles you can watch depend on your connected server."
                    )
                    .foregroundStyle(.secondary)
                }
                ForEach(OnboardingPoster.allCases) { film in
                    Section(film.title) {
                        Text(film.credit)
                        Link("Original poster and attribution", destination: film.source)
                        Link(
                            "Creative Commons Attribution " + film.licenseVersion,
                            destination: URL(
                                string: "https://creativecommons.org/licenses/by/"
                                    + film.licenseVersion + "/")!)
                        Text(
                            "Scaled down and arranged for display. The poster image is shown in full. No endorsement of Mello TV is implied."
                        )
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle("Artwork credits")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } }
            }
        }
    }
}
