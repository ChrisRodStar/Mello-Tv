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
