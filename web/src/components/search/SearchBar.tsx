import { useEffect, useRef } from 'react';

import { Icon } from '../common/Icon';

export function SearchBar({
  query,
  onQueryChange,
}: {
  query: string;
  onQueryChange: (query: string) => void;
}) {
  const searchInput = useRef<HTMLInputElement>(null);
  useEffect(() => {
    searchInput.current?.focus();
  }, []);
  return (
    <section id="catalog-search" className="search-view-header" aria-label="Search catalog">
      <div className="search-box">
        <span className="search-box-icon" aria-hidden="true">
          <Icon name="search" />
        </span>
        <input
          ref={searchInput}
          aria-label="Search movies and TV shows"
          type="search"
          maxLength={200}
          autoComplete="off"
          autoCorrect="off"
          spellCheck={false}
          placeholder="Movies, shows, and more"
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
        />
        <button
          type="button"
          className="search-clear"
          aria-label="Clear search"
          disabled={!query}
          aria-hidden={!query}
          onClick={() => {
            onQueryChange('');
            searchInput.current?.focus();
          }}
        >
          <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <path d="M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zm3.71 12.29a1 1 0 0 1-1.42 1.42L12 13.41l-2.29 2.3a1 1 0 0 1-1.42-1.42L10.59 12l-2.3-2.29a1 1 0 0 1 1.42-1.42L12 10.59l2.29-2.3a1 1 0 0 1 1.42 1.42L13.41 12l2.3 2.29z" />
          </svg>
        </button>
      </div>
    </section>
  );
}
