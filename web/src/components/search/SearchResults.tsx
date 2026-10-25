import type { Title } from '../../model';

import { useSearch } from '../../hooks/useSearch';
import { TitleCard } from '../catalog/TitleCard';
import { Icon } from '../common/Icon';

export function SearchResults({
  query,
  onSelect,
}: {
  query: string;
  onSelect: (title: Title) => void;
}) {
  const { titles, loading, hasLoaded, loadingMore, error, hasMore, loadMore, retry } =
    useSearch(query);

  if (!query) {
    return (
      <div className="search-idle-state">
        <h2>What are you in the mood for?</h2>
        <p>Find a favorite. Discover your next one.</p>
      </div>
    );
  }

  if (loading && !hasLoaded)
    return (
      <div role="status" aria-label="Searching movies and TV shows">
        <div className="loading-grid">
          {Array.from({ length: 6 }, (_, index) => (
            <div className="skeleton-poster" key={index} />
          ))}
        </div>
      </div>
    );

  if (error && !titles.length)
    return (
      <div className="empty-state search-state-card" role="alert">
        <h2>Search unavailable</h2>
        <button type="button" className="search-more" onClick={retry}>
          Try again
        </button>
      </div>
    );

  if (!titles.length && !hasMore)
    return (
      <div className="empty-state search-state-card" aria-busy={loading}>
        <span className="search-state-icon" aria-hidden="true">
          <Icon name="search" />
        </span>
        <h2>No titles found</h2>
        <p className="search-empty-caption">
          No matches for &ldquo;{query}&rdquo;. Check the spelling or try another title.
        </p>
      </div>
    );

  return (
    <section
      className="search-results"
      data-updating={loading}
      aria-label="Search results"
      aria-busy={loading}
    >
      {loading && (
        <span className="sr-only" role="status">
          Updating results…
        </span>
      )}
      <div className="search-results-header">
        <span className="search-results-heading">Results</span>
      </div>
      <div className="results-grid">
        {titles.map((title) => (
          <TitleCard key={`${title.type}-${title.id}`} title={title} onSelect={onSelect} />
        ))}
      </div>
      <div className="search-pagination">
        {error ? (
          <div role="alert">
            <span>
              {error === 'more' ? 'Couldn’t load more titles.' : 'Search unavailable.'}
            </span>
            <button type="button" className="search-more" onClick={retry}>
              Try again
            </button>
          </div>
        ) : (
          hasMore && (
            <button type="button" className="search-more" disabled={loading} onClick={loadMore}>
              {loadingMore ? 'Loading…' : 'Load more'}
            </button>
          )
        )}
      </div>
    </section>
  );
}
