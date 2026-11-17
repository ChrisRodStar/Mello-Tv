import { Fragment, memo, useMemo, useState } from 'react';

import type { Catalog, ServiceHealth, Title, WatchProgress } from '../../model';

import type { Category } from '../../types';
import { ServiceUnavailable } from '../common/ServiceUnavailable';
import { ContinueWatching } from './ContinueWatching';
import { FeaturedCarousel } from './FeaturedCarousel';
import { CategoryDialog } from './CategoryDialog';
import {
  EditorialSpotlightCard,
  NumberedShelf,
  PanoramicShelf,
  Shelf,
} from './Shelf';

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
  onPlay?: (title: Title) => void;
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
  onPlay,
}: CatalogContentProps) {
  const [openShelf, setOpenShelf] = useState<{ id: string; heading: string } | null>(null);

  const movieTitles = useMemo(() => titles.filter((title) => title.type === 'movie'), [titles]);

  const tvTitles = useMemo(() => titles.filter((title) => title.type === 'tv'), [titles]);

  const highestRatedTitles = useMemo(
    () => [...titles].filter((title) => title.rating > 0).sort((a, b) => b.rating - a.rating),
    [titles],
  );

  const sections = useMemo(() => {
    if (catalog?.sections?.length) {
      return catalog.sections.filter((section) => section.titles.length > 0);
    }
    const fallback: Array<{ id: string; heading: string; titles: Title[] }> = [
      { id: 'trending', heading: 'Trending this week', titles },
    ];
    if (filter === 'all') {
      if (movieTitles.length) {
        fallback.push({ id: 'movies', heading: 'Movies', titles: movieTitles });
      }
      if (tvTitles.length) {
        fallback.push({ id: 'tv', heading: 'TV Shows', titles: tvTitles });
      }
    } else if (highestRatedTitles.length) {
      fallback.push({
        id: 'highest-rated',
        heading: 'Highest rated this week',
        titles: highestRatedTitles,
      });
    }
    return fallback;
  }, [catalog?.sections, titles, filter, movieTitles, tvTitles, highestRatedTitles]);

  const featuredTitles = catalog?.featured ?? [];
  const spotlightPick =
    sections.length > 1 && featuredTitles.length >= 8
      ? featuredTitles[featuredTitles.length - 1]
      : undefined;

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
      {featuredTitles.length > 0 && (
        <FeaturedCarousel
          key={filter}
          titles={spotlightPick ? featuredTitles.slice(0, -1) : featuredTitles}
          onSelect={onSelect}
        />
      )}
      {profileId && onResume && (
        <ContinueWatching
          key={profileId}
          profileId={profileId}
          filter={filter}
          onSelect={onSelect}
          onResume={onResume}
        />
      )}
      {sections.map((section, index) => {
        const Row =
          section.id === 'trending' && section.titles.length >= 4
            ? NumberedShelf
            : index % 2 === 1
              ? PanoramicShelf
              : Shelf;
        return (
          <Fragment key={section.id}>
            <Row
              heading={section.heading}
              titles={section.titles}
              onSelect={onSelect}
              onViewMore={profileId && catalog.sections?.length ? () => setOpenShelf(section) : undefined}
            />
            {index === 1 && spotlightPick && (
              <EditorialSpotlightCard title={spotlightPick} onSelect={onSelect} onPlay={onPlay} />
            )}
          </Fragment>
        );
      })}
      {openShelf && profileId && (
        <CategoryDialog
          profileId={profileId}
          category={filter}
          shelf={openShelf}
          onClose={() => setOpenShelf(null)}
          onSelect={onSelect}
        />
      )}
    </>
  );
});
