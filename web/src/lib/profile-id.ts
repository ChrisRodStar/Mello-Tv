import { createUUID } from './uuid';

export function createProfileId() {
  const id = createUUID();
  if (id) return id;
  // Profile identifiers are not authentication tokens.
  return `profile-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}-${Math.random().toString(36).slice(2)}`;
}
