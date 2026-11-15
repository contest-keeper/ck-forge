// The only place that talks to the Rust side of the window.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open, save } from '@tauri-apps/plugin-dialog';
import type { Profile } from '../core/profile';

export type Checked = { problems: string[]; warnings: string[] };

export type SignedIn = { email: string; registry: string | null };

export type DiskView = {
  number: number;
  name: string;
  serial: string;
  size: string;
  bus: string;
  refused: string | null;
};

export type Progress = { done: number; total: number; phase: 'building' | 'writing' | 'checking' };

export const newProfile = () => invoke<Profile>('new_profile');
export const openProfile = (path: string) => invoke<Profile>('open_profile', { path });
export const saveProfile = (path: string, profile: Profile) =>
  invoke<null>('save_profile', { path, profile });
export const checkProfile = (profile: Profile) => invoke<Checked>('check_profile', { profile });

export const listDisks = (base: string, allowVirtual: boolean) =>
  invoke<DiskView[]>('list_disks', { base, allowVirtual });

/**
 * Writes to disks (all at the same time, with one Windows permission prompt) or, for a test
 * without a stick, to a file.
 */
export const writeStick = (
  base: string,
  profilePath: string,
  target: { disks: number[] } | { file: string },
  allowVirtual: boolean,
) =>
  invoke<string>('write_stick', {
    base,
    profilePath,
    disks: 'disks' in target ? target.disks : [],
    toFile: 'file' in target ? target.file : null,
    allowVirtual,
  });

export const hubSignIn = (profilePath: string | null) =>
  invoke<SignedIn>('hub_sign_in', { profilePath });
export const hubSignOut = () => invoke<null>('hub_sign_out');
export const hubStatus = () => invoke<string | null>('hub_status');

export function onProgress(handler: (progress: Progress) => void): Promise<() => void> {
  return listen<Progress>('progress', (event) => handler(event.payload));
}

const JSON_FILES = [{ name: 'Contest profile', extensions: ['json'] }];
const IMAGE_FILES = [{ name: 'Disk image', extensions: ['img'] }];

export const chooseProfileToOpen = () => open({ filters: JSON_FILES });
export const chooseProfileToSave = () =>
  save({ filters: JSON_FILES, defaultPath: 'contest-profile.json' });
export const chooseImageToOpen = () => open({ filters: IMAGE_FILES });
export const chooseImageToSave = () => save({ filters: IMAGE_FILES, defaultPath: 'stick.img' });
