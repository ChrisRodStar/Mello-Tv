import AppKit
import AVFoundation

@main
struct GeneratePlaybackFixture {
    static func main() async throws {
        let output = URL(fileURLWithPath: CommandLine.arguments[1])
        let writer = try AVAssetWriter(outputURL: output, fileType: .mp4)
        let input = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: 640, AVVideoHeightKey: 360])
        let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input,
            sourcePixelBufferAttributes: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32ARGB,
                kCVPixelBufferWidthKey as String: 640, kCVPixelBufferHeightKey as String: 360])
        writer.add(input)
        writer.startWriting()
        writer.startSession(atSourceTime: .zero)
        for frame in 0..<600 {
            while !input.isReadyForMoreMediaData { try await Task.sleep(for: .milliseconds(2)) }
            var buffer: CVPixelBuffer?
            CVPixelBufferPoolCreatePixelBuffer(nil, adaptor.pixelBufferPool!, &buffer)
            let pixel = buffer!
            CVPixelBufferLockBaseAddress(pixel, [])
            let context = CGContext(data: CVPixelBufferGetBaseAddress(pixel), width: 640, height: 360,
                bitsPerComponent: 8, bytesPerRow: CVPixelBufferGetBytesPerRow(pixel), space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.noneSkipFirst.rawValue)!
            context.setFillColor(NSColor(calibratedHue: CGFloat(frame % 600) / 600, saturation: 0.6, brightness: 0.5, alpha: 1).cgColor)
            context.fill(CGRect(x: 0, y: 0, width: 640, height: 360))
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
            ("OFFLINE PLAYBACK FIXTURE\n\(frame / 10) seconds" as NSString).draw(at: NSPoint(x: 65, y: 170),
                withAttributes: [.font: NSFont.boldSystemFont(ofSize: 26), .foregroundColor: NSColor.white])
            NSGraphicsContext.restoreGraphicsState()
            CVPixelBufferUnlockBaseAddress(pixel, [])
            if !adaptor.append(pixel, withPresentationTime: CMTime(value: Int64(frame), timescale: 10)) {
                throw writer.error ?? NSError(domain: "Fixture", code: 1)
            }
        }
        input.markAsFinished()
        await writer.finishWriting()
        guard writer.status == .completed else { throw writer.error ?? NSError(domain: "Fixture", code: 2) }
        print("Generated synthetic 60-second H.264 fixture")
    }
}
