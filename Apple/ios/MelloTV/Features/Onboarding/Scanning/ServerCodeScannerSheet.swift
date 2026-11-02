import AVFoundation
import SwiftUI
import UIKit
import VisionKit

struct ServerCodeScannerSheet: View {
    var onEnterAddress: () -> Void
    var onScan: (String) -> Void

    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    @State private var isReady = false
    @State private var message: String?
    @State private var showsSettings = false

    var body: some View {
        NavigationStack {
            VStack(spacing: 20) {
                if isReady {
                    ServerCodeCamera(onScan: onScan) { _ in
                        isReady = false
                        message =
                            "The camera couldn’t start. You can enter your server address instead."
                    }
                    .clipShape(.rect(cornerRadius: 24))
                    Text(
                        "Point your camera at the QR code shown in Mello TV on your Mac. Scanning connects to that server."
                    )
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                } else if let message {
                    ContentUnavailableView {
                        Label("Scan server code", systemImage: "qrcode.viewfinder")
                    } description: {
                        Text(message)
                    } actions: {
                        if showsSettings {
                            Button("Open Settings") {
                                if let url = URL(string: UIApplication.openSettingsURLString) {
                                    openURL(url)
                                }
                            }
                        }
                        Button("Enter address instead", action: onEnterAddress)
                    }
                } else {
                    ProgressView("Preparing camera")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .padding(24)
            .navigationTitle("Scan server code")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
            .task { await prepareCamera() }
        }
    }

    // MARK: - Camera Permission

    private func prepareCamera() async {
        guard DataScannerViewController.isSupported else {
            message =
                "QR scanning needs a supported iPhone camera. You can still choose a nearby server or enter its address."
            return
        }
        let authorized: Bool
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized: authorized = true
        case .notDetermined: authorized = await AVCaptureDevice.requestAccess(for: .video)
        default: authorized = false
        }
        guard !Task.isCancelled else { return }
        guard authorized else {
            message =
                "Allow camera access in Settings to scan your server’s code, or enter its address instead."
            showsSettings = true
            return
        }
        guard DataScannerViewController.isAvailable else {
            message =
                "The camera is unavailable right now. Try again, or enter your server address instead."
            return
        }
        isReady = true
    }
}
