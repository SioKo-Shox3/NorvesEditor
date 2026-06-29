// @vitest-environment jsdom
/**
 * AssetInspectorPanel tests — Phase B offline manifest details.
 */

import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import type { AssetManifestPayload } from '@norves/bridge-ui';
import type { BridgeState } from '../../state/store.js';
import { assetKeyForEntry, INITIAL_STATE } from '../../state/store.js';

// -------------------------------------------------------------------------
// Mock BridgeContext
// -------------------------------------------------------------------------

let mockState: BridgeState = { ...INITIAL_STATE };

vi.mock('../../state/BridgeContext.js', () => ({
  useBridgeState:    () => mockState,
  useBridgeDispatch: () => vi.fn(),
}));

import { AssetInspectorPanel } from '../AssetInspectorPanel.js';

afterEach(cleanup);
beforeEach(() => {
  mockState = { ...INITIAL_STATE };
});

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function makeDockviewProps(): any { return {}; }

const DEMO_MANIFEST: AssetManifestPayload = {
  version: 1,
  manifestPath: 'C:/Project/manifest.json',
  assets: [
    {
      logicalPath: 'Textures/Silver/silver_albedo.png',
      kind: 'texture',
      variant: 'default',
      format: 'nvtex.v0.rgba8.srgb',
      sourceHash: '23076f79a2789c30',
      cookedPackage: 'Cooked/Silver/silver_albedo.nvpkg',
      entryName: 'Textures/Silver/silver_albedo.nvtex',
      entryType: 'Tex0',
      cookedHash: '0404f473feb0ab26',
      cookedVersion: 0,
    },
  ],
};

describe('AssetInspectorPanel — degradation states', () => {
  it('shows a no-selection placeholder', () => {
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
    };
    render(<AssetInspectorPanel {...makeDockviewProps()} />);
    expect(screen.getByText(/選択なし/)).toBeTruthy();
    expect(screen.getByText(/Asset Browser でアセットを選択してください/)).toBeTruthy();
  });

  it('shows a missing-entry placeholder when the selected key is absent', () => {
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
      selectedAssetKey: assetKeyForEntry({ logicalPath: 'missing.asset', variant: undefined }),
    };
    render(<AssetInspectorPanel {...makeDockviewProps()} />);
    expect(screen.getByText(/アセットが見つかりません/)).toBeTruthy();
  });
});

describe('AssetInspectorPanel — read-only fields', () => {
  it('renders every manifest field for the selected asset', () => {
    const entry = DEMO_MANIFEST.assets[0]!;
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
      selectedAssetKey: assetKeyForEntry(entry),
    };
    render(<AssetInspectorPanel {...makeDockviewProps()} />);

    for (const name of [
      'logicalPath',
      'kind',
      'variant',
      'format',
      'sourceHash',
      'cookedPackage',
      'entryName',
      'entryType',
      'cookedHash',
      'cookedVersion',
    ]) {
      expect(screen.getByText(name)).toBeTruthy();
    }

    expect(screen.getAllByText('Textures/Silver/silver_albedo.png').length).toBeGreaterThan(0);
    expect(screen.getAllByText('texture').length).toBeGreaterThan(0);
    expect(screen.getAllByText('default').length).toBeGreaterThan(0);
    expect(screen.getByText('nvtex.v0.rgba8.srgb')).toBeTruthy();
    expect(screen.getByText('23076f79a2789c30')).toBeTruthy();
    expect(screen.getByText('Cooked/Silver/silver_albedo.nvpkg')).toBeTruthy();
    expect(screen.getByText('Textures/Silver/silver_albedo.nvtex')).toBeTruthy();
    expect(screen.getByText('Tex0')).toBeTruthy();
    expect(screen.getByText('0404f473feb0ab26')).toBeTruthy();
    expect(screen.getByText('0')).toBeTruthy();
    expect(screen.getAllByText('read-only')).toHaveLength(10);
  });

  it('renders missing optional fields as 未設定', () => {
    const looseManifest: AssetManifestPayload = {
      version: 1,
      manifestPath: 'C:/Project/manifest.json',
      assets: [{ logicalPath: 'Textures/Loose.png', kind: 'texture' }],
    };
    const entry = looseManifest.assets[0]!;
    mockState = {
      ...INITIAL_STATE,
      assetManifest: looseManifest,
      selectedAssetKey: assetKeyForEntry(entry),
    };
    render(<AssetInspectorPanel {...makeDockviewProps()} />);
    expect(screen.getAllByText('未設定').length).toBeGreaterThan(0);
  });
});
