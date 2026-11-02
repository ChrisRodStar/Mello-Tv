import MelloKit
import SwiftUI

struct SettingsAppearanceSection: View {
    @AppStorage("app_appearance_mode") private var appearanceMode: AppearanceMode = .system
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    var body: some View {
        let layout =
            dynamicTypeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 8))
            : AnyLayout(HStackLayout(alignment: .center, spacing: 12))
        return layout {
            Text("Appearance")
                .font(.body)
                .foregroundStyle(MelloTVTheme.text)
                .frame(maxWidth: .infinity, alignment: .leading)
            Picker("Appearance", selection: $appearanceMode) {
                ForEach(AppearanceMode.allCases) { mode in
                    Text(mode.rawValue).tag(mode)
                }
            }
            .labelsHidden()
            .pickerStyle(.menu)
            .tint(MelloTVTheme.textSecondary)
            .frame(minHeight: 44)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 8)
        .appleNeumorphic(cornerRadius: 20, elevation: .subtle)
    }
}
