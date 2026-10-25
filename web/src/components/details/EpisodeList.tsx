import type { Episode, WatchProgress } from '../../model';

import { EpisodeRow } from './EpisodeRow';

export function EpisodeList({
  episodes,
  onPlay,
  onPreload,
  progress = [],
  resumeEpisodeNumber,
}: {
  episodes: Episode[];
  progress?: WatchProgress[];
  resumeEpisodeNumber?: number;
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
          isNextUp={resumeEpisodeNumber !== undefined && resumeEpisodeNumber === episode.number}
          onPlay={onPlay}
          onPreload={onPreload}
        />
      ))}
    </ol>
  );
}
