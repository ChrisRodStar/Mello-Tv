import Foundation

public enum TimeFormatting {
    /// Formats seconds into an uptime string, e.g., "45s", "12m 30s", "3h 15m", or "2d 5h".
    public static func formatUptime(_ seconds: Int) -> String {
        guard seconds >= 0 else { return "0s" }
        if seconds < 60 { return "\(seconds)s" }
        if seconds < 3600 { return "\(seconds / 60)m \(seconds % 60)s" }
        if seconds < 86400 { return "\(seconds / 3600)h \((seconds % 3600) / 60)m" }
        return "\(seconds / 86400)d \((seconds % 86400) / 3600)h"
    }

    /// Formats playback or media duration into "M:SS" or "H:MM:SS" (e.g., "3:45", "1:15:30").
    public static func formatDuration(_ totalSeconds: Double) -> String {
        guard !totalSeconds.isNaN, !totalSeconds.isInfinite, totalSeconds >= 0 else { return "0:00" }
        let total = Int(totalSeconds)
        let hours = total / 3600
        let minutes = (total % 3600) / 60
        let seconds = total % 60
        if hours > 0 {
            return String(format: "%d:%02d:%02d", hours, minutes, seconds)
        }
        return String(format: "%d:%02d", minutes, seconds)
    }

    /// Formats remaining time for media cards (e.g., "<1m", "45m", "1h 15m").
    public static func formatRemaining(position: Double, duration: Double) -> String {
        guard duration.isFinite, duration > 0, position.isFinite, position >= 0 else { return "Resume" }
        let fraction = min(1, max(0, position / duration))
        let remainingMinutes = ceil(duration * (1 - fraction) / 60)
        guard remainingMinutes < Double(Int.max) else { return "Resume" }
        let value = Int(remainingMinutes)
        if value < 1 { return "<1m" }
        return value >= 60 ? "\(value / 60)h \(value % 60)m" : "\(value)m"
    }
}
