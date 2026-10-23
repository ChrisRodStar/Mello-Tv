import { memo, useCallback, useLayoutEffect, useRef } from 'react';

import type { Title } from '../../model';

import { Icon } from '../common/Icon';
import { TitleCard } from './TitleCard';

export const Shelf = memo(function Shelf({
  heading,
  titles,
  onSelect,
}: {
  heading: string;
  titles: Title[];
  onSelect: (title: Title) => void;
}) {
  const track = useRef<HTMLDivElement>(null);
  const titleKey = titles.map((title) => `${title.type}-${title.id}`).join(',');
  const titleCount = titles.length;

  useLayoutEffect(() => {
    const element = track.current;
    if (!element || !titleCount) return;
    let timer: ReturnType<typeof setTimeout>;
    function cycleWidth() {
      const first = element!.children[0] as HTMLElement | undefined;
      const middle = element!.children[titleCount] as HTMLElement | undefined;
      return first && middle ? middle.offsetLeft - first.offsetLeft : 0;
    }
    function edgePeek() {
      const first = element!.children[0] as HTMLElement | undefined;
      return first ? first.getBoundingClientRect().width * 0.24 : 0;
    }
    function normalize() {
      const width = cycleWidth();
      if (!width) return;
      const start = width - edgePeek();
      if (element!.scrollLeft < start || element!.scrollLeft >= start + width) {
        element!.scrollTo({
          left: start + ((((element!.scrollLeft - start) % width) + width) % width),
          behavior: 'instant',
        });
      }
    }
    const initialize = () =>
      element.scrollTo({
        left: cycleWidth() - edgePeek(),
        behavior: 'instant',
      });
    initialize();
    const update = () => {
      clearTimeout(timer);
      timer = setTimeout(normalize, 140);
    };
    const observer = new ResizeObserver(initialize);
    observer.observe(element);
    element.addEventListener('scroll', update, { passive: true });
    return () => {
      clearTimeout(timer);
      observer.disconnect();
      element.removeEventListener('scroll', update);
    };
  }, [titleKey, titleCount]);

  const scroll = useCallback((direction: number) => {
    const element = track.current;
    if (element)
      element.scrollBy({
        left: direction * element.clientWidth * 0.85,
        behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches
          ? 'instant'
          : 'smooth',
      });
  }, []);

  if (!titles.length) return null;

  return (
    <section className="shelf" aria-label={heading}>
      <div className="shelf-heading">
        <h2>{heading}</h2>
        <div className="shelf-controls">
          <button aria-label={`Previous titles in ${heading}`} onClick={() => scroll(-1)}>
            <Icon name="left" />
          </button>
          <button aria-label={`Next titles in ${heading}`} onClick={() => scroll(1)}>
            <Icon name="right" />
          </button>
        </div>
      </div>
      <div className="shelf-track" ref={track}>
        {[0, 1, 2].flatMap((copy) =>
          titles.map((title) => (
            <TitleCard
              key={`${copy}-${title.type}-${title.id}`}
              title={title}
              repeated={copy !== 1}
              onSelect={onSelect}
            />
          )),
        )}
      </div>
    </section>
  );
});
