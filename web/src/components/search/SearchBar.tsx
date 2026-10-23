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
        <input
          ref={searchInput}
          aria-label="Search movies and TV shows"
          type="search"
          maxLength={200}
          autoComplete="off"
          autoCorrect="off"
          spellCheck={false}
          placeholder="Search movies and TV shows"
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
        />
        <button
          className="search-clear"
          aria-label="Clear search"
          disabled={!query}
          aria-hidden={!query}
          onClick={() => onQueryChange('')}
        >
          <Icon name="close" />
        </button>
      </div>
    </section>
  );
}
