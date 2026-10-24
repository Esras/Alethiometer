/** A merge staged on the People index (`m`): written when you leave People (`people.merge`). */
export interface StagedMerge {
  keep: number;
  gone: number;
  keepName: string;
  goneName: string;
  /** Fields where the merged-away note's value wins. */
  take: string[];
  /** The notes as the merge dialog read them: if either changed since, nothing is written. */
  keep_text: string;
  gone_text: string;
}

export type SortKey = 'last' | 'signals' | 'meeting' | 'name';
