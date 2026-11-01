// Types shared with the Rust backend (serde camelCase) for the non-query commands.

export interface RefreshResult {
  newMatches: number;
  failed: { folder: string; error: string }[];
}

export type ReplayDirInfo =
  | { status: 'missing' }
  | { status: 'empty' }
  | { status: 'ready'; matchCount: number };
