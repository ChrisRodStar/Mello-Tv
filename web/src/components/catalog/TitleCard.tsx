import { memo, useEffect, useState } from 'react';

import type { Title } from '../../model';

import { Icon } from '../common/Icon';

export function BrandTVMark({ className = '' }: { className?: string }) {
  return (
    <span className={`brand-tv-mark${className ? ` ${className}` : ''}`} aria-hidden="true">
      <svg viewBox="181 208 662 589" fill="currentColor" aria-hidden="true">
        <path
          fillRule="evenodd"
          d="M272 208H752Q843 208 843 299V651Q843 742 752 742H272Q181 742 181 651V299Q181 208 272 208ZM433 379C433 361 450 351 465 361L626 469C639 478 639 491 626 500L465 605C450 615 433 605 433 587ZM483 683H541Q550 683 550 692Q550 701 541 701H483Q474 701 474 692Q474 683 483 683Z"
        />
        <path d="M372 734H652C652 741 665 745 678 752C687 758 692 768 692 779C692 790 686 797 675 797H349C338 797 332 790 334 779C336 763 345 754 357 749C368 744 372 740 372 734Z" />
      </svg>
      <span>TV</span>
    </span>
  );
}

export function CardDetailsGlyph() {
  return (
    <span className="card-details-glyph card-details-glyph-circle" aria-hidden="true">
      <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
        <circle cx="3.5" cy="8" r="1.4" />
        <circle cx="8" cy="8" r="1.4" />
        <circle cx="12.5" cy="8" r="1.4" />
      </svg>
    </span>
  );
}

export function MediaPlaceholderIcon({ type }: { type: 'movie' | 'tv' }) {
  return (
    <span className="placeholder-glyph" aria-hidden="true">
      <Icon name={type === 'movie' ? 'film-slate' : 'television-simple'} />
    </span>
  );
}

export const TitleCard = memo(function TitleCard({
  title,
  onSelect,
  repeated = false,
}: {
  title: Title;
  onSelect: (title: Title) => void;
  repeated?: boolean;
}) {
  const candidates = [title.posterUrl, title.backdropUrl].filter(
    (u): u is string => Boolean(u),
  );
  const [failedCount, setFailedCount] = useState(0);

  useEffect(() => {
    setFailedCount(0);
  }, [title.posterUrl, title.backdropUrl]);

  const activeSrc = candidates[failedCount] ?? null;

  return (
    <button
      type="button"
      className="title-card"
      tabIndex={repeated ? -1 : undefined}
      onClick={() => onSelect(title)}
      aria-label={`Details for ${title.title}`}
    >
      <div className="poster-wrap">
        {activeSrc ? (
          <img
            src={activeSrc}
            alt=""
            decoding="async"
            onError={() => setFailedCount((c) => c + 1)}
          />
        ) : (
          <div className="poster-placeholder">
            <MediaPlaceholderIcon type={title.type} />
            <span>{title.title}</span>
          </div>
        )}
        <BrandTVMark />
      </div>
    </button>
  );
});
