// Test-owned macOS player. Only private-file commands; no source URL in argv/UI.
import AppKit
import AVFoundation
import AVKit

func frameGapIncludingTail(maximum: Double, lastFrame: Double, start: Double, now: Double) -> Double {
    return max(maximum, now - (lastFrame > 0 ? lastFrame : start))
}


struct Command: Decodable {
    let sequence: Int
    let url: String
    let position: Double
    let seconds: Double
    let action: String
}

final class PlayerOwner: NSObject, AVPlayerItemLegibleOutputPushDelegate {
    let player = AVPlayer()
    let view = AVPlayerView(frame: NSRect(x: 0, y: 0, width: 960, height: 540))
    let subtitles = NSTextField(labelWithString: "")
    let window: NSWindow
    let commandPath: String
    let port: Int
    let deadline = ProcessInfo.processInfo.systemUptime + 600
    var timer: Timer?
    var sequence = 0
    var started = 0.0
    var requested = 0.0
    var duration = 0.0
    var seekCompleted = false
    var seeking = false
    var frames = 0
    var cues = 0
    var renderedCues = 0
    var audioOptions = 0
    var subtitleOptions = 0
    var output: AVPlayerItemVideoOutput?
    var legible: AVPlayerItemLegibleOutput?
    var lastFrameTime = CMTime.invalid
    var currentURL = ""
    var reported = false
    var baseline = 0.0
    var commandTime = 0.0
    var firstFrameMS = -1.0
    var lastFrameAt = 0.0
    var maxFrameGap = 0.0

    init(path: String, port: Int) {
        self.commandPath = path
        self.port = port
        window = NSWindow(contentRect: view.frame, styleMask: [.titled, .closable], backing: .buffered, defer: false)
        super.init()
        window.title = "Family Stream — owned acceptance player"
        view.player = player
        view.controlsStyle = .none
        window.contentView = view
        subtitles.frame = NSRect(x: 30, y: 20, width: 900, height: 90)
        subtitles.alignment = .center
        subtitles.font = .systemFont(ofSize: 25, weight: .semibold)
        subtitles.textColor = .white
        subtitles.backgroundColor = .black
        subtitles.drawsBackground = true
        subtitles.maximumNumberOfLines = 3
        subtitles.isHidden = true
        view.addSubview(subtitles)
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApplication.shared.activate(ignoringOtherApps: true)
        timer = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in self?.tick() }
    }

    func emit(_ value: [String: Any]) {
        if let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]) {
            FileHandle.standardOutput.write(data + Data([10]))
        }
    }

    func stop() {
        timer?.invalidate()
        player.pause()
        player.replaceCurrentItem(with: nil)
        NSApplication.shared.terminate(nil)
    }

    func reject(_ stage: String = "command") {
        emit(["event": "rejected", "stage": stage, "code": (player.currentItem?.error as NSError?)?.code ?? 0])
        stop()
    }

    func tick() {
        let now = ProcessInfo.processInfo.systemUptime
        if now >= deadline || !window.isVisible { stop(); return }
        // Commands are atomically replaced by the owner in a private directory.
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: commandPath),
              (attributes[.posixPermissions] as? NSNumber)?.intValue == 0o600,
              (attributes[.size] as? NSNumber)?.intValue ?? 20000 <= 16384,
              attributes[.type] as? FileAttributeType == .typeRegular,
              let data = FileManager.default.contents(atPath: commandPath),
              let command = try? JSONDecoder().decode(Command.self, from: data) else { reject(); return }
        if command.sequence > sequence {
            guard command.sequence == sequence + 1, ["load", "seek", "observe"].contains(command.action), command.position.isFinite,
                  command.position >= 0, command.position <= 7200,
                  command.seconds.isFinite, command.seconds >= 1, command.seconds <= 120,
                  let url = URL(string: command.url), url.scheme == "http",
                  url.host == "127.0.0.1", url.port == port,
                  url.user == nil, url.password == nil, url.fragment == nil,
                  url.path.hasPrefix("/stream/") else { reject(); return }
            sequence = command.sequence
            requested = command.position
            duration = command.seconds
            baseline = command.action == "observe" ? player.currentTime().seconds : command.position
            started = now
            commandTime = now
            firstFrameMS = -1
            lastFrameAt = 0
            maxFrameGap = 0
            reported = false
            seekCompleted = command.action == "observe"
            seeking = command.action == "observe"
            frames = 0
            cues = 0
            renderedCues = 0
            subtitles.stringValue = ""
            subtitles.isHidden = true
            lastFrameTime = .invalid
            if command.action != "load" {
                guard currentURL == command.url else { reject(); return }
            } else {
                currentURL = command.url
                let item = AVPlayerItem(url: url)
                item.preferredForwardBufferDuration = 3
                let videoOutput = AVPlayerItemVideoOutput(pixelBufferAttributes: nil)
                output = videoOutput
                item.add(videoOutput)
                let subtitleOutput = AVPlayerItemLegibleOutput()
                legible = subtitleOutput
                subtitleOutput.suppressesPlayerRendering = true
                subtitleOutput.setDelegate(self, queue: .main)
                item.add(subtitleOutput)
                player.replaceCurrentItem(with: item)
            }
        }
        guard let item = player.currentItem else { return }
        if item.status == .failed { reject("item"); return }
        if !reported && now - started > duration + 40 { reject("deadline"); return }
        if item.status != .readyToPlay { return }
        if !seeking {
            seeking = true
            audioOptions = item.asset.mediaSelectionGroup(forMediaCharacteristic: .audible)?.options.count ?? 0
            if let group = item.asset.mediaSelectionGroup(forMediaCharacteristic: .legible) {
                subtitleOptions = group.options.count
                if let option = group.options.first { item.select(option, in: group) }
            } else { subtitleOptions = 0 }
            player.seek(to: CMTime(seconds: requested, preferredTimescale: 600), toleranceBefore: .zero, toleranceAfter: .zero) { [weak self] done in
                guard let self = self else { return }
                self.seekCompleted = done
                self.started = ProcessInfo.processInfo.systemUptime
                self.player.play()
            }
        }
        if seekCompleted, let output = output {
            let itemTime = player.currentTime()
            if output.hasNewPixelBuffer(forItemTime: itemTime),
               output.copyPixelBuffer(forItemTime: itemTime, itemTimeForDisplay: nil) != nil,
               CMTimeCompare(itemTime, lastFrameTime) != 0 {
                if frames == 0 { firstFrameMS = (now - commandTime) * 1000 }
                if lastFrameAt > 0 { maxFrameGap = max(maxFrameGap, now - lastFrameAt) }
                lastFrameAt = now
                frames += 1
                lastFrameTime = itemTime
            }
            if !reported && now - started >= duration {
                let position = itemTime.seconds
                maxFrameGap = frameGapIncludingTail(maximum: maxFrameGap, lastFrame: lastFrameAt, start: started, now: now)
                emit(["event": "played", "sequence": sequence, "requested_s": requested,
                      "position_s": position.isFinite ? position : -1, "started_s": baseline, "frames_observed": frames,
                      "seek_completed": seekCompleted, "view_ready": view.isReadyForDisplay,
                      "first_frame_ms": firstFrameMS, "max_frame_gap_s": maxFrameGap,
                      "audio_options": audioOptions, "subtitle_options": subtitleOptions,
                      "subtitle_cues": cues, "subtitle_overlay_renders": renderedCues,
                      "rendered_subtitles": renderedCues > 0])
                // Keep playing between commands, including the provider outage.
                reported = true
            }
        }
    }

    func legibleOutput(_ output: AVPlayerItemLegibleOutput, didOutputAttributedStrings strings: [NSAttributedString], nativeSampleBuffers: [Any], forItemTime itemTime: CMTime) {
        // Render source cues in this owned player's visible subtitle overlay.
        // Verify white glyph rasterization in memory; discard the bitmap and text
        // at replacement/exit. Never persist captures or print subtitle content.
        let text = strings.map { $0.string }.joined(separator: "\n")
        subtitles.stringValue = String(text.prefix(2048))
        subtitles.isHidden = text.isEmpty
        if text.isEmpty { return }
        cues += 1
        subtitles.displayIfNeeded()
        guard window.isVisible,
              let bitmap = subtitles.bitmapImageRepForCachingDisplay(in: subtitles.bounds) else { return }
        subtitles.cacheDisplay(in: subtitles.bounds, to: bitmap)
        var glyphPixels = 0
        for row in 0..<bitmap.pixelsHigh {
            for column in 0..<bitmap.pixelsWide {
                if let color = bitmap.colorAt(x: column, y: row)?.usingColorSpace(.deviceRGB),
                   color.redComponent > 0.8, color.greenComponent > 0.8,
                   color.blueComponent > 0.8, color.alphaComponent > 0.8 {
                    glyphPixels += 1
                }
            }
        }
        if glyphPixels >= 20 { renderedCues += 1 }
    }
}

if CommandLine.arguments == [CommandLine.arguments[0], "--self-test-timing"] {
    let trailingFreeze = frameGapIncludingTail(maximum: 0.1, lastFrame: 10, start: 1, now: 20)
    let noFrames = frameGapIncludingTail(maximum: 0, lastFrame: 0, start: 10, now: 20)
    if trailingFreeze != 10 || noFrames != 10 { exit(1) }
    print("{\"timing_regression\":true}")
    exit(0)
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
if CommandLine.arguments.count != 3 || Int(CommandLine.arguments[2]) == nil {
    exit(1)
}
let owner = PlayerOwner(path: CommandLine.arguments[1], port: Int(CommandLine.arguments[2])!)
app.run()
