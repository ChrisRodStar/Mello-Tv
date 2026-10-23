import { loadDetails, loadEpisodes } from './catalog';
import { detailsSchema, episodesSchema, type WatchProgress } from '../model';

import type { PlaybackRequest } from './playback';

export async function resumePlayback(
  item: WatchProgress,
  profileId: string,
): Promise<PlaybackRequest> {
  const base = {
    titleId: item.title.id,
    title: item.title.title,
    profileId,
    catalogTitle: item.title,
  };
  if (item.title.type === 'movie') return { ...base, type: 'movie' };
  let season = item.season;
  let episode = item.episode;
  let episodeTitle = item.episodeTitle;
  if (item.completed) {
    const signal = AbortSignal.timeout(10000);
    const episodes = await loadEpisodes(item.title.id, season, signal);
    let next = episodes.find((candidate) => candidate.number > episode);
    if (!next) {
      const details = await loadDetails(item.title, signal);
      const nextSeason = details.seasons
        .filter((candidate) => candidate.number > season && candidate.episodeCount > 0)
        .sort((a, b) => a.number - b.number)[0];
      if (!nextSeason) throw new Error('You’ve finished the available episodes.');
      season = nextSeason.number;
      next = (await loadEpisodes(item.title.id, season, signal))[0];
    }
    if (!next) throw new Error('No next episode is available.');
    episode = next.number;
    episodeTitle = next.name;
  }
  return { ...base, type: 'tv', season, episode, episodeTitle };
}
