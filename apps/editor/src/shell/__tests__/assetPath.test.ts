import { describe, expect, it } from 'vitest';
import { normalizeLogicalAssetPath } from '../assetPath.js';

describe('normalizeLogicalAssetPath', () => {
  it('accepts Assets-prefixed paths', () => {
    expect(normalizeLogicalAssetPath('Assets/textures/hero.png')).toBe('textures/hero.png');
  });

  it('accepts paths without the Assets prefix', () => {
    expect(normalizeLogicalAssetPath('textures/hero.png')).toBe('textures/hero.png');
  });

  it('collapses a leading dot segment before Assets', () => {
    expect(normalizeLogicalAssetPath('./Assets/a/b.png')).toBe('a/b.png');
  });

  it('collapses dot segments', () => {
    expect(normalizeLogicalAssetPath('Assets/a/./b.png')).toBe('a/b.png');
  });

  it('rejects drive absolute paths', () => {
    expect(() => normalizeLogicalAssetPath('C:/abs/x.png')).toThrow('absolute path');
  });

  it('rejects root absolute paths', () => {
    expect(() => normalizeLogicalAssetPath('/abs/x.png')).toThrow('root absolute');
  });

  it('rejects parent segments inside a path', () => {
    expect(() => normalizeLogicalAssetPath('a/../b.png')).toThrow('contains ..');
  });

  it('rejects parent segments escaping the root', () => {
    expect(() => normalizeLogicalAssetPath('../escape.png')).toThrow('escapes root');
  });

  it('rejects UNC paths', () => {
    expect(() => normalizeLogicalAssetPath('//server/share/x')).toThrow('UNC');
  });

  it('rejects drive-relative paths', () => {
    expect(() => normalizeLogicalAssetPath('C:rel.png')).toThrow('drive-relative');
  });

  it('rejects empty input', () => {
    expect(() => normalizeLogicalAssetPath('')).toThrow('empty');
  });
});
