import { describe, expect, it } from 'vitest';
import { converter } from 'culori';
import { fillWidth, shade, tone, topicBox } from './topicTone';

const oklch = converter('oklch');

describe('topic tones', () => {
  it('puts every hue at one lightness with capped chroma, inside sRGB', () => {
    // The user's category colours: orange, red, aqua, green, yellow, blue.
    for (const hex of ['#fe8019', '#fb4934', '#56b6c2', '#b8bb26', '#fabd2f', '#83a598']) {
      const c = oklch(tone(hex, 0.47, 0.12))!;
      expect(c.l).toBeCloseTo(0.47, 1);
      expect(c.c).toBeLessThanOrEqual(0.121);
      expect(tone(hex, 0.47, 0.12)).toMatch(/^#[0-9a-f]{6}$/);
    }
    const { bg, fill } = topicBox('#fe8019');
    expect(oklch(bg)!.l).toBeLessThan(oklch(fill)!.l);
  });

  it('keeps a grey grey', () => {
    expect(oklch(tone('#928374', 0.47, 0.12))!.c).toBeLessThan(0.03);
  });

  it('fills by length against the busiest box, never invisibly', () => {
    expect(fillWidth(31, 31)).toBe(100);
    expect(fillWidth(6, 31)).toBe(19);
    expect(fillWidth(0, 31)).toBe(5);
    expect(fillWidth(40, 31)).toBe(100);
    expect(fillWidth(3, 0)).toBe(5);
  });

  it('shades a week in four steps against the busiest week', () => {
    expect([0, 1, 2, 3, 4, 5, 8].map((n) => shade(n, 8))).toEqual([0, 1, 1, 2, 2, 3, 4]);
    expect(shade(1, 0)).toBe(0);
  });
});
