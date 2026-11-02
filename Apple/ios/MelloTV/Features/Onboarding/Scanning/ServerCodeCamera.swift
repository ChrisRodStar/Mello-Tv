import SwiftUI
import VisionKit

struct ServerCodeCamera: UIViewControllerRepresentable {
    var onScan: (String) -> Void
    var onFailure: (Error) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(onScan: onScan, onFailure: onFailure) }

    func makeUIViewController(context: Context) -> DataScannerViewController {
        let scanner = DataScannerViewController(
            recognizedDataTypes: [.barcode(symbologies: [.qr])],
            qualityLevel: .balanced,
            recognizesMultipleItems: false,
            isHighFrameRateTrackingEnabled: false,
            isPinchToZoomEnabled: true,
            isGuidanceEnabled: true,
            isHighlightingEnabled: true
        )
        scanner.delegate = context.coordinator
        return scanner
    }

    func updateUIViewController(_ scanner: DataScannerViewController, context: Context) {
        guard !scanner.isScanning, !context.coordinator.finished else { return }
        do { try scanner.startScanning() } catch {
            context.coordinator.finished = true
            Task { @MainActor in onFailure(error) }
        }
    }

    static func dismantleUIViewController(
        _ scanner: DataScannerViewController, coordinator: Coordinator
    ) {
        scanner.stopScanning()
        scanner.delegate = nil
    }

    // MARK: - Scanner Delegate

    final class Coordinator: NSObject, DataScannerViewControllerDelegate {
        var finished = false
        let onScan: (String) -> Void
        let onFailure: (Error) -> Void

        init(onScan: @escaping (String) -> Void, onFailure: @escaping (Error) -> Void) {
            self.onScan = onScan
            self.onFailure = onFailure
        }

        func dataScanner(
            _ scanner: DataScannerViewController, didAdd addedItems: [RecognizedItem],
            allItems: [RecognizedItem]
        ) {
            guard !finished else { return }
            for item in addedItems {
                if case .barcode(let barcode) = item, let value = barcode.payloadStringValue {
                    finished = true
                    scanner.stopScanning()
                    onScan(value)
                    return
                }
            }
        }

        func dataScanner(
            _ scanner: DataScannerViewController,
            becameUnavailableWithError error: DataScannerViewController.ScanningUnavailable
        ) {
            guard !finished else { return }
            finished = true
            scanner.stopScanning()
            onFailure(error)
        }
    }
}
