import MelloKit
import SwiftUI

struct OnboardingConnectionScreen<Navigation: View>: View {
    // MARK: - Connection Inputs

    let profileStore: ProfileStore
    let wide: Bool
    let isConnected: Bool
    let isBusy: Bool
    let isConnectingManual: Bool
    @Binding var manualServerAddress: String
    @Binding var manualConnectError: String?
    @Binding var showsManualAddress: Bool
    let addressIsFocused: FocusState<Bool>.Binding

    // MARK: - Parent Actions

    var onScan: () -> Void
    var onSearch: () -> Void
    var onSelect: (DiscoveredServer) -> Void
    var onConnect: () -> Void
    let navigation: Navigation

    // MARK: - Screen Layout

    var body: some View {
        let layout =
            wide
            ? AnyLayout(HStackLayout(alignment: .top, spacing: 48))
            : AnyLayout(VStackLayout(alignment: .leading, spacing: 28))
        return layout {
            VStack(alignment: .leading, spacing: 28) {
                VStack(alignment: .leading, spacing: 12) {
                    Text(isConnected ? "Make yourself at home." : "Bring your library.")
                        .font(.largeTitle.bold())
                        .tracking(-1.2)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityAddTraits(.isHeader)
                    Text(
                        isConnected
                            ? "Your server is connected. Choose a profile next and start browsing."
                            : "Connect to the Mello TV server you use. Scan its code or choose one on your network."
                    )
                    .font(.body)
                    .foregroundStyle(MelloTVTheme.textSecondary)
                    .lineSpacing(3)
                    .fixedSize(horizontal: false, vertical: true)
                }
                OnboardingNearbyServers(
                    profileStore: profileStore,
                    isConnected: isConnected,
                    isBusy: isBusy,
                    onSearch: onSearch,
                    onSelect: onSelect
                )
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            VStack(alignment: .leading, spacing: 24) {
                Button(action: onScan) {
                    HStack(alignment: .center, spacing: 18) {
                        Image(systemName: "qrcode.viewfinder")
                            .font(.system(size: 34, weight: .light))
                            .accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Scan server code").font(.headline)
                            Text("Find the QR code in Mello TV on your Mac.")
                                .font(.subheadline)
                                .foregroundStyle(MelloTVTheme.textSecondary)
                        }
                        .fixedSize(horizontal: false, vertical: true)
                        Spacer(minLength: 0)
                        Image(systemName: "chevron.right").font(.caption.weight(.semibold))
                            .accessibilityHidden(true)
                    }
                    .frame(maxWidth: .infinity, minHeight: 72, alignment: .leading)
                    .padding(20)
                    .contentShape(Rectangle())
                }
                .buttonStyle(AppleNeumorphicButtonStyle())
                .disabled(isBusy)

                DisclosureGroup(isExpanded: $showsManualAddress) {
                    OnboardingManualConnectionForm(
                        manualServerAddress: $manualServerAddress,
                        addressIsFocused: addressIsFocused,
                        isConnectingManual: isConnectingManual,
                        isBusy: isBusy,
                        onConnect: onConnect,
                        onPaste: { manualConnectError = nil }
                    )
                    .padding(.top, 16)
                } label: {
                    Text("Enter an address instead").font(.subheadline.weight(.semibold))
                        .frame(minHeight: 44)
                }
                .tint(MelloTVTheme.textSecondary)

                if let message = manualConnectError ?? profileStore.errorMessage, !isBusy {
                    Text(message)
                        .font(.footnote)
                        .foregroundStyle(MelloTVTheme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityLabel("Connection error: \(message)")
                }
                if wide { navigation }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

}
