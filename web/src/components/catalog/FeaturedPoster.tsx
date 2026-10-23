import { memo } from 'react';

import type { Title } from '../../model';

export const FeaturedPoster = memo(function FeaturedPoster({
  title,
  active,
  revealed = false,
  eager,
  onActivate,
}: {
  title: Title;
  active: boolean;
  revealed?: boolean;
  eager: boolean;
  onActivate: () => void;
}) {
  return (
    <button
      className={`featured-poster${revealed ? ' is-revealed' : ''}`}
      tabIndex={active ? 0 : -1}
      aria-label={active ? `Details for ${title.title}` : `Feature ${title.title}`}
      onClick={onActivate}
    >
      {title.backdropUrl || title.posterUrl ? (
        <img
          src={title.backdropUrl || title.posterUrl || ''}
          alt=""
          loading={eager ? 'eager' : 'lazy'}
          decoding="async"
        />
      ) : (
        <span className="poster-placeholder">{title.title}</span>
      )}
      <span className="featured-overlay">
        <span className="featured-title">{title.title}</span>
        <span className="featured-meta">
          {title.year && <span>{title.year}</span>}
          <span>{title.type === 'movie' ? 'Movie' : 'Series'}</span>
          {title.rating > 0 && <span>{title.rating.toFixed(1)} / 10</span>}
        </span>
        {title.overview && <span className="featured-overview">{title.overview}</span>}
      </span>
    </button>
  );
});
