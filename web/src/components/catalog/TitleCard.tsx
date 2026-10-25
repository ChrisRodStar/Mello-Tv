import { memo } from 'react';

import type { Title } from '../../model';

export const TitleCard = memo(function TitleCard({
  title,
  onSelect,
  repeated = false,
}: {
  title: Title;
  onSelect: (title: Title) => void;
  repeated?: boolean;
}) {
  return (
    <button
      className="title-card"
      tabIndex={repeated ? -1 : undefined}
      onClick={() => onSelect(title)}
      aria-label={`Details for ${title.title}`}
    >
      <div className="poster-wrap">
        {title.posterUrl ? (
          <img src={title.posterUrl} alt="" loading="lazy" decoding="async" />
        ) : (
          <div className="poster-placeholder">{title.title}</div>
        )}
        <span className="brand-tv-mark" aria-hidden="true">
          <svg viewBox="0 0 16 14" fill="currentColor" aria-hidden="true">
            <rect x="1" y="2" width="14" height="9" rx="2" fill="none" stroke="currentColor" strokeWidth="1.5" />
            <path d="M5 13h6" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
          <span>TV</span>
        </span>
        <div className="card-overlay">
          <h3>{title.title}</h3>
          <div className="card-meta">
            {title.year && <span>{title.year}</span>}
            <span>{title.type === 'movie' ? 'Movie' : 'Series'}</span>
            {title.rating > 0 && (
              <span className="rating">
                {title.rating.toFixed(1)}
                <span className="rating-max"> / 10</span>
              </span>
            )}
          </div>
        </div>
      </div>
    </button>
  );
});
