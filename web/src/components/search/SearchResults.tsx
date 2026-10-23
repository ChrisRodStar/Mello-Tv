import type { Title } from '../../model';

import { useSearch } from '../../hooks/useSearch';
import { TitleCard } from '../catalog/TitleCard';

export function SearchResults({
  query,
  onSelect,
}: {
  query: string;
  onSelect: (title: Title) => void;
}) {
  const { titles, loading, hasLoaded, loadingMore, error, hasMore, loadMore, retry } =
    useSearch(query);
  if (!query) return null;
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
      <div className="empty-state" role="alert">
        <h2>Search unavailable</h2>
        <button className="search-more" onClick={retry}>
          Try again
        </button>
      </div>
    );
  if (!titles.length && !hasMore)
    return (
      <div className="empty-state" aria-busy={loading}>
        <h2>No titles found</h2>
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
            <button className="search-more" onClick={retry}>
              Try again
            </button>
          </div>
        ) : (
          hasMore && (
            <button className="search-more" disabled={loading} onClick={loadMore}>
              {loadingMore ? 'Loading…' : 'Load more'}
            </button>
          )
        )}
      </div>
    </section>
  );
}
