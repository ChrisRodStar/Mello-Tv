import { memo, useEffect, useState } from 'react';

import type { Title } from '../../model';

import { Icon } from '../common/Icon';
import {
  BrandTVMark,
  CardDetailsGlyph,
  MediaPlaceholderIcon,
  TitleCard,
} from './TitleCard';

type ShelfProps = {
  heading: string;
  titles: Title[];
  onSelect: (title: Title) => void;
  onViewMore?: () => void;
};

function ShelfHeading({ heading, onViewMore }: Pick<ShelfProps, 'heading' | 'onViewMore'>) {
  return (
    <div className="shelf-heading">
      <h2>{heading}</h2>
      {onViewMore && (
        <button
          type="button"
          className="shelf-view-more"
          aria-label={`View more ${heading}`}
          onClick={onViewMore}
        >
          View more <Icon name="right" />
        </button>
      )}
    </div>
  );
}

export const Shelf = memo(function Shelf({
  heading,
  titles,
  onSelect,
  onViewMore,
}: ShelfProps) {
  if (!titles.length) return null;

  return (
    <section className="shelf" aria-label={heading}>
      <ShelfHeading heading={heading} onViewMore={onViewMore} />
      <div className="shelf-track">
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
  onViewMore,
}: ShelfProps) {
  if (!titles.length) return null;

  return (
    <section className="shelf numbered-shelf" aria-label={heading}>
      <ShelfHeading heading={heading} onViewMore={onViewMore} />
      <div className="numbered-shelf-track">
        {titles.map((title, index) => (
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
  const candidates = [
    title.backdropUrl ? title.backdropUrl.replace('/t/p/w1280/', '/t/p/w780/') : null,
    title.backdropUrl,
    title.posterUrl,
  ].filter((u, idx, arr): u is string => Boolean(u) && arr.indexOf(u) === idx);
  const [failedCount, setFailedCount] = useState(0);

  useEffect(() => {
    setFailedCount(0);
  }, [title.backdropUrl, title.posterUrl]);

  const artworkUrl = candidates[failedCount] ?? null;

  return (
    <button
      type="button"
      className="panoramic-card"
      onClick={() => onSelect(title)}
      aria-label={`Details for ${title.title}`}
    >
      <div className="panoramic-wrap">
        {artworkUrl ? (
          <img
            src={artworkUrl}
            alt=""
            loading="lazy"
            decoding="async"
            onError={() => setFailedCount((c) => c + 1)}
          />
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
  onViewMore,
}: ShelfProps) {
  if (!titles.length) return null;

  return (
    <section className="shelf panoramic-shelf" aria-label={heading}>
      <ShelfHeading heading={heading} onViewMore={onViewMore} />
      <div className="panoramic-shelf-track">
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
  const candidates = [title.backdropUrl, title.posterUrl].filter(
    (u): u is string => Boolean(u),
  );
  const [failedCount, setFailedCount] = useState(0);

  useEffect(() => {
    setFailedCount(0);
  }, [title.backdropUrl, title.posterUrl]);

  const artworkUrl = candidates[failedCount] ?? null;

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
            <img
              src={artworkUrl}
              alt=""
              decoding="async"
              onError={() => setFailedCount((c) => c + 1)}
            />
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
