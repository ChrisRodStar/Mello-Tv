import { useLayoutEffect, useRef, useState, type CSSProperties } from 'react';

import type { ProfilePreferences, Title } from '../../model';
import { FavoritePoster } from './FavoritePoster';

export function PreferenceTitlePicker({
  titles,
  selected,
  busy,
  onToggle,
}: {
  titles: Title[];
  selected: ProfilePreferences['likedTitles'];
  busy: boolean;
  onToggle: (title: Title) => void;
}) {
  const container = useRef<HTMLDivElement>(null);
  const [layout, setLayout] = useState({ columns: 3, rows: 2 });
  const [page, setPage] = useState(0);
  useLayoutEffect(() => {
    const node = container.current;
    if (!node) return;
    const measure = () => {
      const { width, height } = node.getBoundingClientRect();
      const columns = width >= 950 ? 6 : width >= 600 ? 4 : 3;
      const gap = width >= 600 ? 16 : 10;
      const idealHeight = Math.min(160, (width - gap * (columns - 1)) / columns) * 1.5 + 36;
      const rows = Math.max(1, Math.min(3, Math.floor((height + gap) / (idealHeight + gap))));
      setLayout((previous) =>
        previous.columns === columns && previous.rows === rows ? previous : { columns, rows },
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  const size = layout.columns * layout.rows;
  const pages = Math.max(1, Math.ceil(titles.length / size));
  const currentPage = Math.min(page, pages - 1);
  return (
    <div className="preference-title-picker">
      <div
        className="preference-posters"
        ref={container}
        style={
          { '--poster-columns': layout.columns, '--poster-rows': layout.rows } as CSSProperties
        }
        aria-label="Choose at least three favorites"
      >
        {titles.slice(currentPage * size, (currentPage + 1) * size).map((title) => {
          const checked = selected.some(
            (seed) => seed.id === title.id && seed.type === title.type,
          );
          return (
            <button
              key={`${title.type}-${title.id}`}
              className="preference-poster"
              type="button"
              aria-label={title.title}
              aria-pressed={checked}
              disabled={busy}
              onClick={() => onToggle(title)}
            >
              <span className="preference-poster-art">
                {title.posterUrl && (
                  <FavoritePoster key={title.posterUrl} src={title.posterUrl} />
                )}
                {checked && (
                  <span className="preference-poster-check" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                      <path d="m5 12 4 4 10-10" />
                    </svg>
                  </span>
                )}
              </span>
              <span className="preference-poster-name">{title.title}</span>
            </button>
          );
        })}
      </div>
      <div className="preference-pager">
        <button
          type="button"
          className="profile-secondary"
          disabled={busy || currentPage === 0}
          onClick={() => setPage(currentPage - 1)}
          aria-label="Previous titles"
        >
          Previous
        </button>
        <span aria-live="polite">
          {currentPage + 1} / {pages}
        </span>
        <button
          type="button"
          className="profile-secondary"
          disabled={busy || currentPage >= pages - 1}
          onClick={() => setPage(currentPage + 1)}
          aria-label="Next titles"
        >
          Next
        </button>
      </div>
    </div>
  );
}
