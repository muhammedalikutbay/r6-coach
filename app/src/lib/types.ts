// Types shared with the Rust backend (serde camelCase) for the non-query commands.

export interface RefreshResult {
  newMatches: number;
  /** Stored matches re-parsed because their folder changed. */
  updatedMatches?: number;
  /** Folders still being written or not yet ended; retried automatically. */
  pending?: number;
  failed: { folder: string; error: string }[];
}

export type ReplayDirInfo =
  | { status: 'missing' }
  | { status: 'empty' }
  | { status: 'ready'; matchCount: number };
