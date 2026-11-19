// The only place that talks to the Rust side of the window.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open, save } from '@tauri-apps/plugin-dialog';
import type { Profile } from '../core/profile';

export type Checked = { problems: string[]; warnings: string[] };

export type SignedIn = { email: string; registry: string | null };

export type HubProfileItem = { id: string; name: string };

export type LoadedProfile = { profile: Profile; path: string };

export type DiskView = {
  number: number;
  name: string;
  serial: string;
  size: string;
  bus: string;
  refused: string | null;
};

export type Progress = {
  done: number;
  total: number;
  phase: 'downloading' | 'importing' | 'building' | 'writing' | 'checking';
  /** How far the stick is checked; the checking goes on while the writing does. */
  checked: number;
};

export type CatalogItem = { id: string; label: string; disk_gb: number };

export type HubImage = {
  id: string;
  name: string;
  version: string;
  profile: string;
  size: number;
  filename: string | null;
};

export type BaseProgress = {
  step: 'download' | 'convert' | 'provision' | 'patch' | 'hash';
  done: number;
  total: number;
};

export const newProfile = () => invoke<Profile>('new_profile');
export const openProfile = (path: string) => invoke<Profile>('open_profile', { path });
export const saveProfile = (path: string, profile: Profile) =>
  invoke<null>('save_profile', { path, profile });
export const checkProfile = (profile: Profile) => invoke<Checked>('check_profile', { profile });

export const listDisks = (base: string, allowVirtual: boolean) =>
  invoke<DiskView[]>('list_disks', { base, allowVirtual });

export const importBaseImage = (archive: string, out: string, sha256: string) =>
  invoke<null>('import_base_image', { archive, out, sha256 });

export const listHubImages = () => invoke<HubImage[]>('list_hub_images');
export const downloadHubImage = (id: string, out: string) =>
  invoke<null>('download_hub_image', { id, out });
export const enrolmentSupported = (base: string) =>
  invoke<boolean>('enrolment_supported', { base });

/**
 * Writes to disks (all at the same time, with one Windows permission prompt) or, for a test
 * without a stick, to a file.
 */
export const writeStick = (
  base: string,
  profilePath: string,
  target: { disks: number[] } | { file: string },
  allowVirtual: boolean,
  wifiPassword: string,
  firstBootEnrolment: boolean,
) =>
  invoke<string>('write_stick', {
    base,
    profilePath,
    disks: 'disks' in target ? target.disks : [],
    toFile: 'file' in target ? target.file : null,
    allowVirtual,
    wifiPassword: wifiPassword === '' ? null : wifiPassword,
    firstBootEnrolment,
  });

export const listCatalog = () => invoke<CatalogItem[]>('list_catalog');

/** Makes a base image from an official distribution; takes 15 to 40 minutes. Returns its SHA-256. */
export const makeBaseImage = (distro: string, out: string, patch: string | null) =>
  invoke<string>('make_base_image', { distro, out, patch });

export function onBaseProgress(handler: (progress: BaseProgress) => void): Promise<() => void> {
  return listen<BaseProgress>('base-progress', (event) => handler(event.payload));
}

export const hubSignIn = (profilePath: string | null) =>
  invoke<SignedIn>('hub_sign_in', { profilePath });
export const hubSignOut = () => invoke<null>('hub_sign_out');
export const hubStatus = () => invoke<string | null>('hub_status');
export const hubRetireStick = (id: string) => invoke<null>('hub_retire_stick', { id });

export function onProgress(handler: (progress: Progress) => void): Promise<() => void> {
  return listen<Progress>('progress', (event) => handler(event.payload));
}

const JSON_FILES = [{ name: 'Contest profile', extensions: ['json'] }];
const IMAGE_FILES = [{ name: 'Disk image', extensions: ['img'] }];

export const chooseProfileToOpen = () => open({ filters: JSON_FILES });
export const chooseProfileToSave = () =>
  save({ filters: JSON_FILES, defaultPath: 'contest-profile.json' });
export const chooseImageToOpen = () => open({ filters: IMAGE_FILES });
export const chooseImageArchive = () =>
  open({ filters: [{ name: 'Keeper OS download', extensions: ['zst'] }] });
export const chooseBaseToSave = () => save({ filters: IMAGE_FILES, defaultPath: 'keeper-os.img' });
export const choosePatchToOpen = () =>
  open({ filters: [{ name: 'Keeper OS patch', extensions: ['gz'] }] });
export const chooseImageToSave = () => save({ filters: IMAGE_FILES, defaultPath: 'stick.img' });

export const hubListProfiles = () => invoke<HubProfileItem[]>('hub_list_profiles');
export const hubLoadProfile = (id: string) => invoke<LoadedProfile>('hub_load_profile', { id });
export const hubSaveProfile = (path: string, profile: Profile) =>
  invoke<Profile>('hub_save_profile', { path, profile });
