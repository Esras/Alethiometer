import { describe, expect, it } from 'vitest';
import { stepZoom, zoomKey } from './zoom';

describe('stepZoom', () => {
  it('steps like a browser and stops at the ends', () => {
    expect(stepZoom(1, 'in')).toBe(1.1);
    expect(stepZoom(1, 'out')).toBe(0.9);
    expect(stepZoom(2.5, 'in')).toBe(2.5);
    expect(stepZoom(0.5, 'out')).toBe(0.5);
  });
  it('goes from a value set in Settings to the next step its way', () => {
    expect(stepZoom(1.3, 'in')).toBe(1.5);
    expect(stepZoom(1.3, 'out')).toBe(1.25);
  });
  it('resets to 1', () => expect(stepZoom(1.75, 'reset')).toBe(1));
});

describe('zoomKey', () => {
  it('reads the physical key', () => {
    expect(zoomKey({ code: 'Equal', key: '=' })).toBe('in');
    expect(zoomKey({ code: 'Equal', key: '+' })).toBe('in');
    expect(zoomKey({ code: 'Minus', key: '-' })).toBe('out');
    expect(zoomKey({ code: 'Digit0', key: '0' })).toBe('reset');
    expect(zoomKey({ code: 'KeyK', key: 'k' })).toBe(null);
  });
});
