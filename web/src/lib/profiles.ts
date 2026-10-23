import { profileColors, type Profile } from '../model';

export { profileColors };
export type { Profile };
export type ProfileState = { profiles: Profile[]; activeId: string };
export const initialProfiles: ProfileState = { profiles: [], activeId: '' };
