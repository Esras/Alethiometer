import { describe, expect, it } from 'vitest';
import { placeByRow } from './dock';

describe('placeByRow', () => {
  const row = (top: number, bottom: number) => ({ left: 20, top, bottom });
  it('goes below the row, else above it', () => {
    expect(placeByRow(row(100, 130), 320, 260, 1000, 800)).toEqual({ left: 20, top: 134 });
    expect(placeByRow(row(600, 630), 320, 260, 1000, 800)).toEqual({ left: 20, bottom: 204 });
  });
  it('stays on screen when neither side has room', () => {
    // An expanded row from near the top to near the bottom.
    expect(placeByRow(row(200, 700), 320, 260, 1000, 800)).toEqual({ left: 20, top: 532 });
    expect(placeByRow(row(100, 150), 320, 260, 1000, 300)).toEqual({ left: 20, top: 32 });
  });
});
