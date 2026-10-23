import type { Profile } from '../../lib/profiles';

export function ProfileAvatar({ profile }: { profile: Pick<Profile, 'name' | 'color'> }) {
  return (
    <span
      className="profile-avatar"
      style={{ backgroundColor: profile.color }}
      aria-hidden="true"
    >
      {Array.from(profile.name.trim())[0]?.toLocaleUpperCase() || 'F'}
    </span>
  );
}
