import { memo, useEffect, useState, type CSSProperties } from 'react';

import type { Title } from '../../model';

export const FeaturedPoster = memo(function FeaturedPoster({
  title,
  active,
  revealed = false,
  effectiveOffset = 0,
  depth = 0,
  contentOpacity = 1,
  onActivate,
  onDetails,
}: {
  title: Title;
  active: boolean;
  revealed?: boolean;
  eager: boolean;
  effectiveOffset?: number;
  depth?: number;
  contentOpacity?: number;
  onActivate: () => void;
  onDetails?: () => void;
}) {
  const candidates = [title.posterUrl, title.backdropUrl].filter(
    (u): u is string => Boolean(u),
  );
  const [failedCount, setFailedCount] = useState(0);

  useEffect(() => {
    setFailedCount(0);
  }, [title.posterUrl, title.backdropUrl]);

  const imageUrl = candidates[failedCount] ?? null;
  const blurPx = depth > 0 ? Number((depth * 0.8).toFixed(2)) : 0;
  const brightness = depth > 0 ? Number((1 - depth * 0.08).toFixed(3)) : 1;

  const artworkFilter =
    depth > 0.01 ? `blur(${blurPx}px) brightness(${brightness})` : undefined;

  const absOffset = Math.abs(effectiveOffset);

  return (
    <div
      className={`featured-poster-surface${active ? ' is-center' : ''}${revealed ? ' is-revealed' : ''}`}
      style={
        {
          '--poster-depth': depth,
          '--content-opacity': contentOpacity,
        } as CSSProperties
      }
    >
      <button
        type="button"
        className="featured-poster"
        tabIndex={active ? 0 : -1}
        aria-label={active ? `Play ${title.title}` : `Show ${title.title}`}
        disabled={absOffset > 3.2}
        onClick={onActivate}
      >
        {imageUrl ? (
          <img
            className="featured-poster-img"
            src={imageUrl}
            alt=""
            loading="eager"
            decoding="async"
            draggable={false}
            style={artworkFilter ? { filter: artworkFilter } : undefined}
            onError={() => setFailedCount((c) => c + 1)}
          />
        ) : (
          <span className="featured-poster-placeholder">
            <svg
              className="featured-placeholder-icon"
              viewBox="0 0 256 256"
              fill="currentColor"
              aria-hidden="true"
            >
              <path d="M216,104H102.09L210,75.51a8,8,0,0,0,5.68-9.84l-8.16-30a15.93,15.93,0,0,0-19.42-11.13L35.81,64.74a15.75,15.75,0,0,0-9.7,7.4,15.51,15.51,0,0,0-1.55,12L32,111.56c0,.14,0,.29,0,.44v88a16,16,0,0,0,16,16H208a16,16,0,0,0,16-16V112A8,8,0,0,0,216,104ZM136.44,62.72l28.05,16.2L119.3,91.05,91.25,74.85ZM71.55,80.13l28.05,16.2L53.6,108.67,43.8,72.18ZM208,200H48V120H208v80Z" />
            </svg>
            <span className="featured-placeholder-title">{title.title}</span>
          </span>
        )}
        <span className="featured-poster-depth-shade" aria-hidden="true" />
      </button>

      {/* Top-right BrandTVMark watermark */}
      <span
        className="featured-brand-tv-mark"
        style={{ opacity: contentOpacity }}
        aria-hidden="true"
      >
        <svg
          className="featured-brand-tv-icon"
          viewBox="181 208 662 589"
          fill="currentColor"
          aria-hidden="true"
        >
          <path
            fillRule="evenodd"
            d="M272 208H752Q843 208 843 299V651Q843 742 752 742H272Q181 742 181 651V299Q181 208 272 208ZM433 379C433 361 450 351 465 361L626 469C639 478 639 491 626 500L465 605C450 615 433 605 433 587ZM483 683H541Q550 683 550 692Q550 701 541 701H483Q474 701 474 692Q474 683 483 683Z"
          />
          <path d="M372 734H652C652 741 665 745 678 752C687 758 692 768 692 779C692 790 686 797 675 797H349C338 797 332 790 334 779C336 763 345 754 357 749C368 744 372 740 372 734Z" />
        </svg>
        <span className="featured-brand-tv-text">TV</span>
      </span>

      {/* Bottom-right CardDetailsGlyph (•••) */}
      <button
        type="button"
        className="featured-details-glyph"
        tabIndex={active ? 0 : -1}
        aria-label={`Details for ${title.title}`}
        aria-hidden={!active}
        style={{
          opacity: contentOpacity,
          pointerEvents: active ? 'auto' : 'none',
        }}
        onClick={(event) => {
          event.stopPropagation();
          (onDetails ?? onActivate)();
        }}
      >
        <span className="featured-details-circle" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <circle cx="5.5" cy="12" r="2" />
            <circle cx="12" cy="12" r="2" />
            <circle cx="18.5" cy="12" r="2" />
          </svg>
        </span>
      </button>

      {/* Subtle continuous 0.75px rim stroke */}
      <span className="featured-poster-rim" aria-hidden="true" />
    </div>
  );
});
