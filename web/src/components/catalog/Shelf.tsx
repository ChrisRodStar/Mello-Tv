import { memo, useCallback, useRef } from 'react';

import type { Title } from '../../model';

import { Icon } from '../common/Icon';
import {
  BrandTVMark,
  CardDetailsGlyph,
  MediaPlaceholderIcon,
  TitleCard,
} from './TitleCard';

function useHorizontalShelfScroll() {
  const track = useRef<HTMLDivElement>(null);
  const scroll = useCallback((direction: number) => {
    const element = track.current;
    if (!element) return;
    element.scrollBy({
      left: direction * element.clientWidth * 0.85,
      behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches
        ? 'instant'
        : 'smooth',
    });
  }, []);
  return { track, scroll };
}

export const Shelf = memo(function Shelf({
  heading,
  titles,
  onSelect,
}: {
  heading: string;
  titles: Title[];
  onSelect: (title: Title) => void;
}) {
  const { track, scroll } = useHorizontalShelfScroll();

  if (!titles.length) return null;

  return (
    <section className="shelf" aria-label={heading}>
      <div className="shelf-heading">
        <h2>{heading}</h2>
        <div className="shelf-controls">
          <button
            type="button"
            aria-label={`Previous titles in ${heading}`}
            onClick={() => scroll(-1)}
          >
            <Icon name="left" />
          </button>
          <button
            type="button"
            aria-label={`Next titles in ${heading}`}
            onClick={() => scroll(1)}
          >
            <Icon name="right" />
          </button>
        </div>
      </div>
      <div className="shelf-track" ref={track}>
        {titles.map((title) => (
          <TitleCard
            key={`${title.type}-${title.id}`}
            title={title}
            onSelect={onSelect}
          />
        ))}
      </div>
    </section>
  );
});

export const NumberedPosterTile = memo(function NumberedPosterTile({
  rank,
  title,
  onSelect,
}: {
  rank: number;
  title: Title;
  onSelect: (title: Title) => void;
}) {
  return (
    <div className="numbered-poster-tile">
      <span className="numbered-poster-rank" aria-hidden="true">
        {rank}
      </span>
      <TitleCard title={title} onSelect={onSelect} />
    </div>
  );
});

export const NumberedShelf = memo(function NumberedShelf({
  heading,
  titles,
  onSelect,
}: {
  heading: string;
  titles: Title[];
  onSelect: (title: Title) => void;
}) {
  const { track, scroll } = useHorizontalShelfScroll();
  const topTen = titles.slice(0, 10);

  if (!topTen.length) return null;

  return (
    <section className="shelf numbered-shelf" aria-label={heading}>
      <div className="shelf-heading">
        <h2>{heading}</h2>
        <div className="shelf-controls">
          <button
            type="button"
            aria-label={`Previous titles in ${heading}`}
            onClick={() => scroll(-1)}
          >
            <Icon name="left" />
          </button>
          <button
            type="button"
            aria-label={`Next titles in ${heading}`}
            onClick={() => scroll(1)}
          >
            <Icon name="right" />
          </button>
        </div>
      </div>
      <div className="numbered-shelf-track" ref={track}>
        {topTen.map((title, index) => (
          <NumberedPosterTile
            key={`${title.type}-${title.id}`}
            rank={index + 1}
            title={title}
            onSelect={onSelect}
          />
        ))}
      </div>
    </section>
  );
});

export const PanoramicCard = memo(function PanoramicCard({
  title,
  onSelect,
}: {
  title: Title;
  onSelect: (title: Title) => void;
}) {
  const artworkUrl = title.backdropUrl ?? title.posterUrl;
  return (
    <button
      type="button"
      className="panoramic-card"
      onClick={() => onSelect(title)}
      aria-label={`Details for ${title.title}`}
    >
      <div className="panoramic-wrap">
        {artworkUrl ? (
          <img src={artworkUrl} alt="" loading="lazy" decoding="async" />
        ) : (
          <div className="panoramic-placeholder">
            <MediaPlaceholderIcon type={title.type} />
            <span>{title.title}</span>
          </div>
        )}
        <span className="panoramic-scrim" aria-hidden="true" />
        <BrandTVMark />
        <span className="panoramic-title">{title.title}</span>
      </div>
    </button>
  );
});

export const PanoramicShelf = memo(function PanoramicShelf({
  heading,
  titles,
  onSelect,
}: {
  heading: string;
  titles: Title[];
  onSelect: (title: Title) => void;
}) {
  const { track, scroll } = useHorizontalShelfScroll();

  if (!titles.length) return null;

  return (
    <section className="shelf panoramic-shelf" aria-label={heading}>
      <div className="shelf-heading">
        <h2>{heading}</h2>
        <div className="shelf-controls">
          <button
            type="button"
            aria-label={`Previous titles in ${heading}`}
            onClick={() => scroll(-1)}
          >
            <Icon name="left" />
          </button>
          <button
            type="button"
            aria-label={`Next titles in ${heading}`}
            onClick={() => scroll(1)}
          >
            <Icon name="right" />
          </button>
        </div>
      </div>
      <div className="panoramic-shelf-track" ref={track}>
        {titles.map((title) => (
          <PanoramicCard
            key={`${title.type}-${title.id}`}
            title={title}
            onSelect={onSelect}
          />
        ))}
      </div>
    </section>
  );
});

export const EditorialSpotlightCard = memo(function EditorialSpotlightCard({
  title,
  onSelect,
  onPlay,
}: {
  title: Title;
  onSelect: (title: Title) => void;
  onPlay?: (title: Title) => void;
}) {
  const artworkUrl = title.backdropUrl ?? title.posterUrl;
  return (
    <div className="editorial-spotlight-wrap">
      <button
        type="button"
        className="editorial-spotlight-card"
        onClick={() => (onPlay ? onPlay(title) : onSelect(title))}
        aria-label={`Play ${title.title}`}
      >
        <span className="editorial-spotlight-art">
          {artworkUrl ? (
            <img src={artworkUrl} alt="" loading="lazy" decoding="async" />
          ) : (
            <span className="editorial-spotlight-placeholder">
              <MediaPlaceholderIcon type={title.type} />
            </span>
          )}
          <span className="editorial-spotlight-scrim" aria-hidden="true" />
        </span>
        <BrandTVMark />
        <span className="editorial-spotlight-copy">
          <span className="editorial-spotlight-eyebrow">SPOTLIGHT</span>
          <span className="editorial-spotlight-title">{title.title}</span>
        </span>
      </button>
      <button
        type="button"
        className="editorial-spotlight-details"
        onClick={() => onSelect(title)}
        aria-label={`Details for ${title.title}`}
        title="Title details"
      >
        <CardDetailsGlyph />
      </button>
    </div>
  );
});
