import type { Episode } from '../../model';

import { Icon } from '../common/Icon';

export function EpisodeInfo({ episode }: { episode: Episode }) {
  return (
    <details
      className="episode-info-inline"
      onKeyDown={(event) => {
        if (event.key === 'Escape' && event.currentTarget.open) {
          event.stopPropagation();
          event.preventDefault();
          event.currentTarget.open = false;
          event.currentTarget.querySelector('summary')?.focus();
        }
      }}
    >
      <summary aria-label={`Description for ${episode.name}`}>
        Details <Icon name="down" />
      </summary>
      <p>{episode.overview}</p>
    </details>
  );
}
