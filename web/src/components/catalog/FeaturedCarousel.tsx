import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from 'react';

import type { Title } from '../../model';

import { Icon } from '../common/Icon';
import { FeaturedPoster } from './FeaturedPoster';

export const FeaturedCarousel = memo(function FeaturedCarousel({
  titles,
  onSelect,
}: {
  titles: Title[];
  onSelect: (title: Title) => void;
}) {
  const featured = useMemo(() => titles.slice(0, 7), [titles]);
  const [carousel, setCarousel] = useState({ active: 0, previous: 0 });
  const [paused, setPaused] = useState(false);
  const swiped = useRef(false);
  const active = carousel.active;
  const startX = useRef<number | null>(null);
  const index = featured.length ? active % featured.length : 0;

  const choose = useCallback((next: number) => {
    setCarousel((value) => ({ active: next, previous: value.active }));
  }, []);

  const move = useCallback(
    (direction: number) => {
      setCarousel((value) => ({
        active: (value.active + direction + featured.length) % featured.length,
        previous: value.active,
      }));
    },
    [featured.length],
  );

  useEffect(() => {
    if (featured.length <= 1 || paused) return;
    if (
      typeof window !== 'undefined' &&
      window.matchMedia('(prefers-reduced-motion: reduce)').matches
    ) {
      return;
    }

    const timer = setInterval(() => {
      if (typeof document !== 'undefined' && document.hidden) return;
      move(1);
    }, 6500);

    return () => clearInterval(timer);
  }, [featured.length, paused, move]);

  const circularOffset = useCallback(
    (card: number, selected: number) => {
      let offset = (card - selected + featured.length) % featured.length;
      if (offset > featured.length / 2) offset -= featured.length;
      return offset;
    },
    [featured.length],
  );

  if (!featured.length) return null;

  return (
    <section
      className="featured-carousel"
      aria-label="Featured titles"
      aria-roledescription="carousel"
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocusCapture={() => setPaused(true)}
      onBlurCapture={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) {
          setPaused(false);
        }
      }}
    >
      <div
        className="featured-stage"
        onTouchStart={(event) => {
          setPaused(true);
          swiped.current = false;
          startX.current = event.touches[0]?.clientX ?? null;
        }}
        onTouchEnd={(event) => {
          setPaused(false);
          const end = event.changedTouches[0]?.clientX;
          if (
            startX.current !== null &&
            end !== undefined &&
            Math.abs(end - startX.current) > 45
          ) {
            swiped.current = true;
            move(end < startX.current ? 1 : -1);
          }
          startX.current = null;
        }}
      >
        {featured.map((title, cardIndex) => {
          const offset = circularOffset(cardIndex, index);
          const wrapped = Math.abs(offset - circularOffset(cardIndex, carousel.previous)) > 1;
          const current = offset === 0;
          return (
            <article
              key={`${title.type}-${title.id}`}
              className={`featured-card${current ? ' is-active' : ''}${wrapped ? ' is-wrapping' : ''}`}
              style={
                {
                  '--offset': offset,
                  '--distance': Math.abs(offset),
                  zIndex: 5 - Math.abs(offset),
                } as CSSProperties
              }
              aria-hidden={!current}
            >
              <FeaturedPoster
                title={title}
                active={current}
                eager={cardIndex === 0}
                onActivate={() => {
                  if (swiped.current) {
                    swiped.current = false;
                    return;
                  }
                  if (!current) {
                    choose(cardIndex);
                    return;
                  }
                  onSelect(title);
                }}
              />
            </article>
          );
        })}
        {featured.length > 1 && (
          <>
            <button
              className="featured-arrow featured-previous"
              aria-label="Previous featured title"
              onClick={() => move(-1)}
            >
              <Icon name="left" />
            </button>
            <button
              className="featured-arrow featured-next"
              aria-label="Next featured title"
              onClick={() => move(1)}
            >
              <Icon name="right" />
            </button>
          </>
        )}
      </div>
      <div className="featured-pagination">
        <div className="featured-dots" aria-label="Choose featured title">
          {featured.map((title, cardIndex) => (
            <button
              key={`${title.type}-${title.id}`}
              aria-label={`Feature ${title.title}`}
              aria-pressed={index === cardIndex}
              className={index === cardIndex ? 'active' : ''}
              onClick={() => choose(cardIndex)}
            />
          ))}
        </div>
      </div>
    </section>
  );
});
