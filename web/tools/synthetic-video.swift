import AVFoundation
import CoreVideo
import Foundation

let output = URL(fileURLWithPath: CommandLine.arguments[1])
let writer = try AVAssetWriter(outputURL: output, fileType: .mp4)
let input = AVAssetWriterInput(mediaType: .video, outputSettings: [AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: 320, AVVideoHeightKey: 180, AVVideoCompressionPropertiesKey: [AVVideoAverageBitRateKey: 180000]])
let adapter = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32ARGB, kCVPixelBufferWidthKey as String: 320, kCVPixelBufferHeightKey as String: 180])
writer.add(input)
guard writer.startWriting() else { fatalError("Synthetic writer unavailable") }
writer.startSession(atSourceTime: .zero)
for frame in 0..<600 {
    while !input.isReadyForMoreMediaData { Thread.sleep(forTimeInterval: 0.002) }
    var pixel: CVPixelBuffer?
    guard CVPixelBufferPoolCreatePixelBuffer(nil, adapter.pixelBufferPool!, &pixel) == kCVReturnSuccess, let pixel else { fatalError("Synthetic buffer unavailable") }
    CVPixelBufferLockBaseAddress(pixel, [])
    let data = CVPixelBufferGetBaseAddress(pixel)!.assumingMemoryBound(to: UInt8.self)
    let stride = CVPixelBufferGetBytesPerRow(pixel)
    for y in 0..<180 { for x in 0..<320 {
        let i = y*stride+x*4
        data[i]=255; data[i+1]=UInt8((x+frame*3)%255);data[i+2]=UInt8((y+frame)%255);data[i+3]=90
    } }
    CVPixelBufferUnlockBaseAddress(pixel, [])
    guard adapter.append(pixel, withPresentationTime: CMTime(value: Int64(frame), timescale: 30)) else { fatalError("Synthetic frame rejected") }
}
input.markAsFinished()
let done = DispatchSemaphore(value: 0)
writer.finishWriting { done.signal() }
guard done.wait(timeout: .now()+10) == .success, writer.status == .completed else { fatalError("Synthetic completion rejected") }
