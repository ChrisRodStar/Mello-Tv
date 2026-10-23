import type { WatchProgress } from '../../model';

export function WatchIndicator({ progress }: { progress?: WatchProgress }) {
  if (!progress || progress.duration <= 0 || progress.position <= 0) return null;
  const percent = Math.min(100, Math.round((progress.position / progress.duration) * 100));
  return (
    <span className="watch-indicator">
      <span>{percent}% played</span>
      <progress
        value={progress.position}
        max={progress.duration}
        aria-label={`${percent}% played`}
      />
    </span>
  );
}
