import AppKit
import SwiftUI

struct WelcomeOnboardingView: View {
    let port: Int
    var onFinish: () -> Void

    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var step = 0

    private let steps = ["Welcome", "Your server", "Made independently"]

    var body: some View {
        VStack(spacing: 0) {
            header
            Spacer(minLength: 12)
            pageContent
                .id(step)
            Spacer(minLength: 16)
            navigation
        }
        .frame(maxWidth: 440)
        .padding(.horizontal, 36)
        .padding(.top, 32)
        .padding(.bottom, 36)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background { SatinBackdrop().ignoresSafeArea() }
        .foregroundStyle(MelloTVTheme.text)
    }

    private var header: some View {
        HStack(spacing: 8) {
            Image(systemName: "tv.fill")
                .font(.system(size: 16, weight: .semibold))
                .foregroundStyle(MelloTVTheme.accent)

            Text("Mello TV")
                .font(.system(size: 15, weight: .bold))
                .tracking(-0.3)
                .foregroundStyle(MelloTVTheme.text)

            Spacer()

            // Step Indicator Capsules
            HStack(spacing: 5) {
                ForEach(0..<steps.count, id: \.self) { index in
                    Capsule()
                        .fill(index <= step ? MelloTVTheme.accent : MelloTVTheme.border)
                        .frame(width: index == step ? 22 : 8, height: 4)
                        .animation(.spring(response: 0.35, dampingFraction: 0.8), value: step)
                }
            }
        }
        .frame(minHeight: 36)
        .padding(.horizontal, 4)
    }

    private var pageContent: some View {
        VStack(spacing: 24) {
            stepArtwork
                .padding(.top, 8)

            VStack(spacing: 8) {
                Text(stepTitle)
                    .font(.system(size: 28, weight: .bold))
                    .tracking(-0.8)
                    .foregroundStyle(MelloTVTheme.text)

                Text(stepSubtitle)
                    .font(.system(size: 13, weight: .regular))
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .multilineTextAlignment(.center)
                    .lineSpacing(3)
                    .frame(maxWidth: 360)
            }

            // Step-specific detail
            Group {
                if step == 0 {
                    welcomeFeatures
                } else if step == 1 {
                    serverConnectionCard
                } else {
                    supportCard
                }
            }
            .frame(maxWidth: .infinity)
            .padding(.top, 4)
        }
    }

    private var stepTitle: String {
        switch step {
        case 0: return "Settle in."
        case 1: return "Your cinema server."
        default: return "Made independently."
        }
    }

    private var stepSubtitle: String {
        switch step {
        case 0: return "Your movies, your series. A space for everything you love to watch."
        case 1: return "Mello TV runs quietly on this Mac, sharing your library across your home network."
        default: return "An ad-free, quiet experience built with care. Help support what comes next."
        }
    }

    @ViewBuilder
    private var stepArtwork: some View {
        if step == 0 {
            if let icon = NSApp.applicationIconImage {
                Image(nsImage: icon)
                    .resizable()
                    .scaledToFit()
                    .frame(width: 80, height: 80)
                    .shadow(color: Color.black.opacity(0.2), radius: 12, y: 6)
            }
        } else if step == 1 {
            ZStack {
                RoundedRectangle(cornerRadius: 22, style: .continuous)
                    .fill(MelloTVTheme.card)
                    .frame(width: 76, height: 76)
                    .overlay {
                        RoundedRectangle(cornerRadius: 22, style: .continuous)
                            .strokeBorder(MelloTVTheme.border, lineWidth: 1)
                    }
                    .shadow(color: Color.black.opacity(0.1), radius: 8, y: 4)

                Image(systemName: "server.rack")
                    .font(.system(size: 32, weight: .semibold))
                    .foregroundStyle(MelloTVTheme.accent)
            }
        } else {
            ZStack {
                RoundedRectangle(cornerRadius: 22, style: .continuous)
                    .fill(MelloTVTheme.card)
                    .frame(width: 76, height: 76)
                    .overlay {
                        RoundedRectangle(cornerRadius: 22, style: .continuous)
                            .strokeBorder(MelloTVTheme.border, lineWidth: 1)
                    }
                    .shadow(color: Color.black.opacity(0.1), radius: 8, y: 4)

                Image(systemName: "cup.and.saucer.fill")
                    .font(.system(size: 30, weight: .semibold))
                    .foregroundStyle(Color(red: 1.0, green: 0.84, blue: 0.1))
            }
        }
    }

    private var welcomeFeatures: some View {
        VStack(spacing: 14) {
            featureRow(
                icon: "film.stack",
                iconColor: Color(red: 0.25, green: 0.65, blue: 1.0),
                title: "Find your next favorite",
                detail: "Movies and series curated in a quiet, ad-free interface."
            )
            featureRow(
                icon: "person.2.fill",
                iconColor: Color(red: 0.4, green: 0.8, blue: 0.55),
                title: "Make it yours",
                detail: "Personal profiles with tailored recommendations and watch history."
            )
            featureRow(
                icon: "play.circle.fill",
                iconColor: Color(red: 0.95, green: 0.6, blue: 0.2),
                title: "Pick up where you left off",
                detail: "Seamless continue watching across companion devices."
            )
        }
    }

    private func featureRow(icon: String, iconColor: Color, title: String, detail: String) -> some View {
        HStack(alignment: .top, spacing: 14) {
            ZStack {
                RoundedRectangle(cornerRadius: 9, style: .continuous)
                    .fill(iconColor.opacity(0.16))
                    .frame(width: 32, height: 32)
                Image(systemName: icon)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(iconColor)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(MelloTVTheme.text)
                Text(detail)
                    .font(.system(size: 11))
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .lineSpacing(2)
            }
            Spacer(minLength: 0)
        }
        .padding(14)
        .background(MelloTVTheme.card, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(MelloTVTheme.border, lineWidth: 1)
        }
    }

    private var serverConnectionCard: some View {
        VStack(spacing: 12) {
            HStack(spacing: 12) {
                Circle()
                    .fill(Color(red: 0.35, green: 0.85, blue: 0.55))
                    .frame(width: 10, height: 10)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Local Server Active")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(MelloTVTheme.text)
                    Text("Broadcasting on port \(port)")
                        .font(.system(size: 11))
                        .foregroundStyle(MelloTVTheme.textSecondary)
                }
                Spacer()
                Text("http://localhost:\(port)")
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .background(MelloTVTheme.sunken, in: RoundedRectangle(cornerRadius: 6))
            }
            .padding(16)
            .background(MelloTVTheme.card, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(MelloTVTheme.border, lineWidth: 1)
            }

            Text("Open the web client or iOS companion app to create a profile and build your catalog. Catalog access is included, with an optional personal key in Settings → Server.")
                .font(.system(size: 11))
                .foregroundStyle(MelloTVTheme.muted)
                .multilineTextAlignment(.center)
        }
    }

    private var supportCard: some View {
        VStack(spacing: 14) {
            Link(destination: AppLinks.buyMeACoffeeURL) {
                HStack(spacing: 8) {
                    Image(systemName: "cup.and.saucer.fill")
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(Color(red: 0.14, green: 0.11, blue: 0))
                    Text("Buy Me a Coffee")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(Color(red: 0.14, green: 0.11, blue: 0))
                }
                .frame(maxWidth: .infinity)
                .frame(height: 44)
                .background(
                    LinearGradient(
                        colors: [Color(red: 1, green: 0.87, blue: 0), Color(red: 0.96, green: 0.73, blue: 0)],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    ),
                    in: RoundedRectangle(cornerRadius: 12, style: .continuous)
                )
                .shadow(color: Color(red: 1, green: 0.8, blue: 0).opacity(0.3), radius: 8, y: 3)
            }
            .buttonStyle(.plain)

            Text("Always optional. Thank you for using Mello TV.")
                .font(.system(size: 11))
                .foregroundStyle(MelloTVTheme.muted)
        }
        .padding(18)
        .background(MelloTVTheme.card, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .strokeBorder(MelloTVTheme.border, lineWidth: 1)
        }
    }

    private var navigation: some View {
        HStack(spacing: 12) {
            if step > 0 {
                Button {
                    withAnimation(reduceMotion ? nil : .spring(response: 0.35, dampingFraction: 0.8)) {
                        step -= 1
                    }
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "chevron.left")
                            .font(.system(size: 11, weight: .semibold))
                        Text("Back")
                            .font(.system(size: 13, weight: .medium))
                    }
                    .frame(height: 40)
                    .padding(.horizontal, 16)
                    .foregroundStyle(MelloTVTheme.text)
                    .background(MelloTVTheme.card, in: Capsule())
                    .overlay {
                        Capsule().strokeBorder(MelloTVTheme.border, lineWidth: 1)
                    }
                }
                .buttonStyle(.plain)
            }

            Button {
                if step < steps.count - 1 {
                    withAnimation(reduceMotion ? nil : .spring(response: 0.35, dampingFraction: 0.8)) {
                        step += 1
                    }
                } else {
                    onFinish()
                }
            } label: {
                Text(step == steps.count - 1 ? "Get Started" : "Continue")
                    .font(.system(size: 13, weight: .semibold))
                    .frame(maxWidth: .infinity)
                    .frame(height: 40)
                    .foregroundStyle(.white)
                    .background(MelloTVTheme.accent, in: Capsule())
                    .shadow(color: MelloTVTheme.accent.opacity(0.35), radius: 8, y: 3)
            }
            .buttonStyle(.plain)
        }
        .padding(.top, 8)
    }
}
