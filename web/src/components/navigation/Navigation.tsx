import { memo } from 'react';

import { BUY_ME_A_COFFEE_URL } from '../../config/links';

import type { Profile } from '../../lib/profiles';
import type { Category } from '../../types';
import { ProfileAvatar } from '../profiles/ProfileAvatar';
import { BrandLogo } from './BrandLogo';
import { NavigationIcon } from './NavigationIcon';

const categories: { id: Category; label: string }[] = [
  { id: 'all', label: 'Home' },
  { id: 'movie', label: 'Movies' },
  { id: 'tv', label: 'TV Shows' },
];

export const Navigation = memo(function Navigation({
  profile,
  filter,
  searchOpen,
  profilesOpen,
  onProfiles,
  onCategoryChange,
  onSearch,
}: {
  profile: Profile;
  filter: Category;
  searchOpen: boolean;
  profilesOpen: boolean;
  onProfiles: () => void;
  onCategoryChange: (category: Category) => void;
  onSearch: () => void;
}) {
  return (
    <nav className="side-navigation" aria-label="Main navigation">
      <button
        type="button"
        className="navigation-brand"
        aria-label="Mello TV Home"
        title="Mello TV"
        onClick={() => onCategoryChange('all')}
      >
        <BrandLogo />
      </button>
      <div className="navigation-browse">
        {categories.map((category) => {
          const isActive = filter === category.id && !searchOpen && !profilesOpen;
          return (
            <button
              key={category.id}
              type="button"
              className={isActive ? 'active' : ''}
              aria-current={isActive ? 'page' : undefined}
              onClick={() => onCategoryChange(category.id)}
            >
              <NavigationIcon
                name={category.id === 'all' ? 'home' : category.id === 'movie' ? 'movie' : 'tv'}
                active={isActive}
              />
              <span>{category.label}</span>
            </button>
          );
        })}
        <button
          type="button"
          className={searchOpen ? 'active' : ''}
          aria-expanded={searchOpen}
          aria-controls="catalog-search"
          onClick={onSearch}
        >
          <NavigationIcon name="search" active={searchOpen} />
          <span>Search</span>
        </button>
      </div>
      <div className="navigation-footer">
        <button
          type="button"
          className={profilesOpen ? 'navigation-profile active' : 'navigation-profile'}
          aria-current={profilesOpen ? 'page' : undefined}
          onClick={onProfiles}
        >
          <ProfileAvatar profile={profile} />
          <span>Profile</span>
        </button>
        <a
          className="navigation-support"
          href={BUY_ME_A_COFFEE_URL}
          target="_blank"
          rel="noopener noreferrer"
        >
          Support Mello TV
        </a>
      </div>
    </nav>
  );
});
