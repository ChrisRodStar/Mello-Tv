import React from 'react';
import { createRoot } from 'react-dom/client';
import { PlaybackView } from '../../src/components/playback/PlaybackView';
import { apiPlaybackResolver, type PlaybackRequest } from '../../src/lib/playback';
import { useProfileLibrary } from '../../src/hooks/useProfileLibrary';
import '../../src/styles.css';

function LibraryFixture() {
  const library = useProfileLibrary('synthetic');
  return (
    <>
      <button onClick={() => void library.refresh()}>Refresh library</button>
      {library.hasMore && (
        <button disabled={library.saving} onClick={() => void library.loadMore()}>
          Load more history
        </button>
      )}
      <output>{library.progress.length}</output>
    </>
  );
}

function Fixture() {
  const [request, setRequest] = React.useState<PlaybackRequest>({
    type: 'tv',
    titleId: 7,
    title: 'Synthetic Series',
    profileId: 'synthetic',
    season: 1,
    episode: 1,
    episodeTitle: 'Episode 1',
  });
  const select = (episode: number) =>
    setRequest((current) => ({
      ...current,
      type: 'tv',
      season: 1,
      episode,
      episodeTitle: `Episode ${episode}`,
      startAt: undefined,
    }));
  (window as any).selectEpisode = select;
  return (
    <PlaybackView
      request={request}
      resolver={apiPlaybackResolver}
      onBack={() => {
        (window as any).exits = ((window as any).exits ?? 0) + 1;
      }}
      onNext={() => select(request.type === 'tv' ? request.episode + 1 : 1)}
    />
  );
}
createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    {new URLSearchParams(location.search).has('library') ? <LibraryFixture /> : <Fixture />}
  </React.StrictMode>,
);
