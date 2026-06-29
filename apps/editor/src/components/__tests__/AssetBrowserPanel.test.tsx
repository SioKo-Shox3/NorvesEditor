// @vitest-environment jsdom
/**
 * AssetBrowserPanel tests — Phase B offline manifest browser.
 */

import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, screen, cleanup, fireEvent } from '@testing-library/react';
import type { AssetManifestPayload } from '@norves/bridge-ui';
import type { BridgeState } from '../../state/store.js';
import { assetKeyForEntry, INITIAL_STATE } from '../../state/store.js';

// -------------------------------------------------------------------------
// Mock BridgeContext + bridge actions
// -------------------------------------------------------------------------

let mockState: BridgeState = { ...INITIAL_STATE };

vi.mock('../../state/BridgeContext.js', () => ({
  useBridgeState:    () => mockState,
  useBridgeDispatch: () => vi.fn(),
}));

const readAssetManifest = vi.fn();
const selectAsset = vi.fn();
const clearAssetManifest = vi.fn();
const dismissError = vi.fn();
const dismissAssetError = vi.fn();

vi.mock('../../hooks/useBridge.js', () => ({
  useBridgeActions: () => ({
    readAssetManifest,
    selectAsset,
    clearAssetManifest,
    dismissError,
    dismissAssetError,
  }),
}));

import { AssetBrowserPanel } from '../AssetBrowserPanel.js';
import { AssetInspectorPanel } from '../AssetInspectorPanel.js';

afterEach(cleanup);
beforeEach(() => {
  mockState = { ...INITIAL_STATE };
  readAssetManifest.mockClear();
  selectAsset.mockClear();
  clearAssetManifest.mockClear();
  dismissError.mockClear();
  dismissAssetError.mockClear();
});

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function makeDockviewProps(): any { return {}; }

const DEMO_MANIFEST: AssetManifestPayload = {
  version: 1,
  manifestPath: 'C:/Project/manifest.json',
  assets: [
    {
      logicalPath: 'textures/hero.png',
      kind: 'texture',
      variant: 'default',
      format: 'png',
      sourceHash: 'source-1',
    },
    {
      logicalPath: 'materials/hero.mat',
      kind: 'material',
      variant: 'mobile',
    },
  ],
};

describe('AssetBrowserPanel — manifest path and loading', () => {
  it('defaults the manifest path to <rootPath>/manifest.json when workspace exists', () => {
    mockState = {
      ...INITIAL_STATE,
      workspace: {
        rootPath: 'C:/Project',
        assetsRoot: 'C:/Project/Assets',
        name: 'Project',
      },
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    const input = screen.getByLabelText('Manifest path') as HTMLInputElement;
    expect(input.value).toBe('C:/Project/manifest.json');
  });

  it('leaves the manifest path empty when no workspace exists', () => {
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    const input = screen.getByLabelText('Manifest path') as HTMLInputElement;
    expect(input.value).toBe('');
  });

  it('calls readAssetManifest with the trimmed input path when Load is clicked', () => {
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    fireEvent.change(screen.getByLabelText('Manifest path'), {
      target: { value: '  C:/Project/custom-manifest.json  ' },
    });
    fireEvent.click(screen.getByText('Load'));
    expect(readAssetManifest).toHaveBeenCalledWith('C:/Project/custom-manifest.json');
  });
});

describe('AssetBrowserPanel — degradation states', () => {
  it('shows an unloaded placeholder before a manifest is loaded', () => {
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    expect(screen.getByText(/manifest 未ロード/)).toBeTruthy();
  });

  it('shows a 0件 placeholder for an empty manifest', () => {
    mockState = {
      ...INITIAL_STATE,
      assetManifest: { version: 1, manifestPath: 'C:/Project/manifest.json', assets: [] },
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    expect(screen.getByText('アセット 0 件')).toBeTruthy();
    expect(screen.getByText(/この manifest にはアセットがありません/)).toBeTruthy();
  });

  it('shows assetError without hiding the browser controls', () => {
    mockState = {
      ...INITIAL_STATE,
      assetError: { kind: 'asset', message: 'manifest parse failed' },
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    expect(screen.getByRole('alert').textContent).toContain('manifest parse failed');
    expect(screen.getByText('Load')).toBeTruthy();
  });

  it('surfaces a failed reload even while a previous manifest is still shown', () => {
    // hasManifest === true AND assetError set: the banner must still appear so
    // the user notices the reload failed instead of silently keeping stale data.
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
      assetError: { kind: 'asset', message: 'broken.json parse failed' },
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    expect(screen.getByRole('alert').textContent).toContain('broken.json parse failed');
    // The previous list remains visible alongside the error.
    expect(screen.getByText('textures/hero.png')).toBeTruthy();
  });

  it('does NOT show an unrelated shared lastError in the asset banner', () => {
    mockState = {
      ...INITIAL_STATE,
      lastError: { kind: 'engine', message: 'unrelated bridge error' },
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    expect(screen.queryByRole('alert')).toBeNull();
  });
});

describe('AssetBrowserPanel — grouped list and selection', () => {
  it('renders assets grouped by kind with logicalPath, variant, and unknown health', () => {
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    expect(screen.getByText('material')).toBeTruthy();
    expect(screen.getByText('texture')).toBeTruthy();
    expect(screen.getByText('textures/hero.png')).toBeTruthy();
    expect(screen.getByText('materials/hero.mat')).toBeTruthy();
    expect(screen.getByText('default')).toBeTruthy();
    expect(screen.getByText('mobile')).toBeTruthy();
    expect(screen.getAllByText('未検証')).toHaveLength(2);
  });

  it('selects the clicked asset key', () => {
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    fireEvent.click(screen.getByText('textures/hero.png'));
    expect(selectAsset).toHaveBeenCalledWith(assetKeyForEntry(DEMO_MANIFEST.assets[0]!));
  });

  it('can drive the Asset Inspector selection display', () => {
    selectAsset.mockImplementation((key: string) => {
      mockState = { ...mockState, selectedAssetKey: key };
    });
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
    };
    const { rerender } = render(
      <>
        <AssetBrowserPanel {...makeDockviewProps()} />
        <AssetInspectorPanel {...makeDockviewProps()} />
      </>,
    );

    expect(screen.getByText(/選択なし/)).toBeTruthy();
    fireEvent.click(screen.getByText('textures/hero.png'));
    rerender(
      <>
        <AssetBrowserPanel {...makeDockviewProps()} />
        <AssetInspectorPanel {...makeDockviewProps()} />
      </>,
    );

    expect(screen.getByText('sourceHash')).toBeTruthy();
    expect(screen.getByText('source-1')).toBeTruthy();
  });

  it('highlights the selected asset row', () => {
    const selectedKey = assetKeyForEntry(DEMO_MANIFEST.assets[1]!);
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
      selectedAssetKey: selectedKey,
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    const selectedRow = screen.getByText('materials/hero.mat').closest('button');
    expect(selectedRow?.className).toContain('scene-node__row--selected');
  });
});
