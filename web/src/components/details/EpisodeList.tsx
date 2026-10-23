import type { Episode, WatchProgress } from '../../model';

import { EpisodeRow } from './EpisodeRow';

export function EpisodeList({
  episodes,
  onPlay,
  onPreload,
  progress = [],
}: {
  episodes: Episode[];
  progress?: WatchProgress[];
  onPlay: (episode: Episode) => void;
  onPreload?: (episode: Episode) => void;
}) {
  return (
    <ol className="episode-list">
      {episodes.map((episode) => (
        <EpisodeRow
          key={episode.number}
          episode={episode}
          progress={progress.find((item) => item.episode === episode.number)}
          onPlay={onPlay}
          onPreload={onPreload}
        />
      ))}
    </ol>
  );
}
