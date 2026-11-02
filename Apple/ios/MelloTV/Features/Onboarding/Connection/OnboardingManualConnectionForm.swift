import MelloKit
import SwiftUI

struct OnboardingManualConnectionForm: View {
    @Binding var manualServerAddress: String
    let addressIsFocused: FocusState<Bool>.Binding
    let isConnectingManual: Bool
    let isBusy: Bool
    var onConnect: () -> Void
    var onPaste: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Server address").font(.headline)
            Text("Use the address shown in your server’s Network settings.")
                .font(.subheadline).foregroundStyle(MelloTVTheme.textSecondary)
            TextField("192.168.1.50:8080", text: $manualServerAddress)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .keyboardType(.URL)
                .submitLabel(.go)
                .focused(addressIsFocused)
                .onSubmit { onConnect() }
                .padding(16)
                .appleNeumorphicRecessed(cornerRadius: 12)
                .accessibilityLabel("Server address")
            PasteButton(payloadType: String.self) { values in
                guard let value = values.first else { return }
                manualServerAddress = value
                onPaste()
            }
            .tint(MelloTVTheme.textSecondary)
            .accessibilityLabel("Paste server address")
            Button(action: onConnect) {
                HStack(spacing: 10) {
                    if isConnectingManual { ProgressView().tint(MelloTVTheme.text) }
                    Text(isConnectingManual ? "Connecting…" : "Connect to server")
                        .font(.subheadline.weight(.semibold))
                }
                .frame(maxWidth: .infinity, minHeight: 50)
                .contentShape(Rectangle())
            }
            .buttonStyle(AppleNeumorphicButtonStyle())
            .disabled(
                manualServerAddress.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                    || isBusy)

        }
    }

}
