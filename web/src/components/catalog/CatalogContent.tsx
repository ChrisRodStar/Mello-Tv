import { memo, useMemo } from 'react';

import type { Catalog, ServiceHealth, Title, WatchProgress } from '../../model';

import type { Category } from '../../types';
import { ServiceUnavailable } from '../common/ServiceUnavailable';
import { ContinueWatching } from './ContinueWatching';
import { FeaturedCarousel } from './FeaturedCarousel';
import { Shelf } from './Shelf';

type CatalogContentProps = {
  catalog: Catalog | null;
  profileId?: string;
  onResume?: (item: WatchProgress) => Promise<void>;
  error: string;
  health: ServiceHealth | null;
  checking: boolean;
  filter: Category;
  titles: Title[];
  retry: () => void;
  onSelect: (title: Title) => void;
};

export const CatalogContent = memo(function CatalogContent({
  catalog,
  profileId,
  onResume,
  error,
  health,
  checking,
  filter,
  titles,
  retry,
  onSelect,
}: CatalogContentProps) {
  const featuredTitles = useMemo(() => {
    return catalog?.sections?.find((section) => section.titles.length)?.titles ?? titles;
  }, [catalog?.sections, titles]);

  const movieTitles = useMemo(() => titles.filter((title) => title.type === 'movie'), [titles]);

  const tvTitles = useMemo(() => titles.filter((title) => title.type === 'tv'), [titles]);

  const highestRatedTitles = useMemo(
    () => [...titles].filter((title) => title.rating > 0).sort((a, b) => b.rating - a.rating),
    [titles],
  );

  return error ? (
    <ServiceUnavailable health={health} checking={checking} onRetry={retry} />
  ) : !catalog ? (
    <div role="status" aria-label="Loading catalog">
      <div className="loading-heading" />
      <div className="loading-grid">
        {Array.from({ length: 6 }, (_, index) => (
          <div className="skeleton-poster" key={index} />
        ))}
      </div>
    </div>
  ) : !catalog.configured ? (
    <div className="empty-state">
      <h2>Catalog unavailable</h2>
    </div>
  ) : !titles.length ? (
    <div className="empty-state">
      <h2>No titles found</h2>
    </div>
  ) : (
    <>
      <FeaturedCarousel key={filter} titles={featuredTitles} onSelect={onSelect} />
      {profileId && onResume && (
        <ContinueWatching
          key={profileId}
          profileId={profileId}
          filter={filter}
          onSelect={onSelect}
          onResume={onResume}
        />
      )}
      {catalog.sections ? (
        catalog.sections.map((section) => (
          <Shelf
            key={section.id}
            heading={section.heading}
            titles={section.titles}
            onSelect={onSelect}
          />
        ))
      ) : (
        <>
          <Shelf heading="Trending this week" titles={titles} onSelect={onSelect} />
          {filter === 'all' ? (
            <>
              <Shelf heading="Movies" titles={movieTitles} onSelect={onSelect} />
              <Shelf heading="TV Shows" titles={tvTitles} onSelect={onSelect} />
            </>
          ) : (
            <Shelf
              heading="Highest rated this week"
              titles={highestRatedTitles}
              onSelect={onSelect}
            />
          )}
        </>
      )}
    </>
  );
});
