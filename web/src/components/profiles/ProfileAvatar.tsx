import type { CSSProperties } from 'react';
import type { Profile } from '../../lib/profiles';

export function ProfileAvatar({
  profile,
  size,
  isSelected = false,
}: {
  profile: Pick<Profile, 'name' | 'color'>;
  size?: number;
  isSelected?: boolean;
}) {
  const initial = Array.from(profile.name.trim())[0]?.toLocaleUpperCase();
  const style: CSSProperties = {
    backgroundColor: profile.color,
    ...(size
      ? {
          width: `${size}px`,
          height: `${size}px`,
          borderRadius: `${Math.round(size * 0.18)}px`,
          fontSize: `${Math.round(size * 0.46)}px`,
        }
      : undefined),
  };

  return (
    <span
      className={isSelected ? 'profile-avatar is-selected' : 'profile-avatar'}
      style={style}
      aria-hidden="true"
    >
      {initial ? (
        <span className="profile-avatar-initial">{initial}</span>
      ) : (
        <svg
          className="profile-avatar-user-icon"
          viewBox="0 0 256 256"
          fill="currentColor"
          aria-hidden="true"
        >
          <path d="M230.93,220a8,8,0,0,1-6.93,4H32a8,8,0,0,1-6.92-12c15.23-26.33,38.7-45.21,66.09-54.16a72,72,0,1,1,73.66,0c27.39,8.95,50.86,27.83,66.09,54.16A8,8,0,0,1,230.93,220Z" />
        </svg>
      )}
    </span>
  );
}
