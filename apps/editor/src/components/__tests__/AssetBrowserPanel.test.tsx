// @vitest-environment jsdom
/**
 * AssetBrowserPanel tests — Phase B offline manifest browser.
 */

import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { render, screen, cleanup, fireEvent } from '@testing-library/react';
import type { AssetManifestPayload, AssetResolveResult } from '@norves/bridge-ui';
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
const reloadAssetRuntime = vi.fn();
const dismissAssetReloadError = vi.fn();

vi.mock('../../hooks/useBridge.js', () => ({
  useBridgeActions: () => ({
    readAssetManifest,
    selectAsset,
    clearAssetManifest,
    dismissError,
    dismissAssetError,
    reloadAssetRuntime,
    dismissAssetReloadError,
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
  reloadAssetRuntime.mockClear();
  dismissAssetReloadError.mockClear();
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

function resolveResult(status: AssetResolveResult['status']): AssetResolveResult {
  return {
    status,
    source: status === 'successCooked' ? 'cooked' : 'none',
    normalizedLogicalPath: 'textures/hero.png',
    reason: status === 'cookedEntryHashMismatch' ? 'cooked hash does not match' : undefined,
  };
}

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

describe('AssetBrowserPanel — runtime manifest reload', () => {
  it.each<[string, BridgeState]>([
    ['disconnected', { ...INITIAL_STATE }],
    [
      'capability absent',
      {
        ...INITIAL_STATE,
        connection: {
          status: 'connected',
          sessionId: 's1',
          capabilityNames: new Set(['asset.read']),
        },
      },
    ],
    [
      'runtime reload unsupported',
      {
        ...INITIAL_STATE,
        connection: {
          status: 'connected',
          sessionId: 's1',
          capabilityNames: new Set(['asset.reload']),
        },
        assetReloadUnsupported: true,
      },
    ],
  ])('keeps Reload Runtime disabled while %s', (_label, state) => {
    mockState = state;
    render(<AssetBrowserPanel {...makeDockviewProps()} />);

    const button = screen.getByRole('button', { name: 'Reload runtime asset manifest' });
    expect(button.textContent).toBe('Reload Runtime');
    expect((button as HTMLButtonElement).disabled).toBe(true);
  });

  it('enables Reload Runtime for a connected engine advertising asset.reload', () => {
    mockState = {
      ...INITIAL_STATE,
      connection: {
        status: 'connected',
        sessionId: 's1',
        capabilityNames: new Set(['asset.reload']),
      },
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);

    const button = screen.getByRole('button', { name: 'Reload runtime asset manifest' });
    expect((button as HTMLButtonElement).disabled).toBe(false);
  });

  it('reloads the runtime manifest once with no arguments and never reads the offline manifest', () => {
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
      connection: {
        status: 'connected',
        sessionId: 's1',
        capabilityNames: new Set(['asset.reload']),
      },
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);

    fireEvent.click(screen.getByRole('button', { name: 'Reload runtime asset manifest' }));

    expect(reloadAssetRuntime).toHaveBeenCalledOnce();
    expect(reloadAssetRuntime).toHaveBeenCalledWith();
    expect(readAssetManifest).not.toHaveBeenCalled();
  });

  it('shows a runtime reload error with its own dismiss action', () => {
    mockState = {
      ...INITIAL_STATE,
      assetReloadError: { kind: 'engine', message: 'runtime reload failed' },
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);

    expect(screen.getByRole('alert').textContent).toContain('runtime reload failed');
    fireEvent.click(
      screen.getByRole('button', { name: 'Dismiss runtime asset manifest reload error' }),
    );
    expect(dismissAssetReloadError).toHaveBeenCalledOnce();
    expect(dismissAssetError).not.toHaveBeenCalled();
  });

  it('keeps offline and runtime errors independently dismissible while preserving the manifest', () => {
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
      assetError: { kind: 'asset', message: 'offline manifest parse failed' },
      assetReloadError: { kind: 'engine', message: 'runtime reload failed' },
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);

    expect(screen.getAllByRole('alert')).toHaveLength(2);
    expect(screen.getByText(/offline manifest parse failed/)).toBeTruthy();
    expect(screen.getByText(/runtime reload failed/)).toBeTruthy();
    expect(screen.getByText('textures/hero.png')).toBeTruthy();

    fireEvent.click(
      screen.getByRole('button', { name: 'Dismiss runtime asset manifest reload error' }),
    );
    expect(dismissAssetReloadError).toHaveBeenCalledOnce();
    expect(dismissAssetError).not.toHaveBeenCalled();
    expect(screen.getByText(/offline manifest parse failed/)).toBeTruthy();
    expect(screen.getByText('textures/hero.png')).toBeTruthy();

    fireEvent.click(
      screen.getByRole('button', { name: 'Dismiss offline asset manifest error' }),
    );
    expect(dismissAssetError).toHaveBeenCalledOnce();
    expect(dismissAssetReloadError).toHaveBeenCalledOnce();
    expect(screen.getByText(/runtime reload failed/)).toBeTruthy();
    expect(screen.getByText('textures/hero.png')).toBeTruthy();
  });
});

describe('AssetBrowserPanel — grouped list and selection', () => {
  it('renders assets grouped by kind with logicalPath, variant, and disconnected health', () => {
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
    expect(screen.getAllByText('未検証(未接続)')).toHaveLength(2);
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

describe('AssetBrowserPanel — live health column', () => {
  it('shows 未対応 for all rows when the engine does not support asset.resolve', () => {
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
      connection: { status: 'connected' },
      assetCapabilitySupported: false,
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    expect(screen.getAllByText('未対応')).toHaveLength(2);
  });

  it('shows selected cooked success while non-selected rows remain 未検証', () => {
    const selectedKey = assetKeyForEntry(DEMO_MANIFEST.assets[0]!);
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
      selectedAssetKey: selectedKey,
      connection: { status: 'connected' },
      assetResolveByKey: {
        [selectedKey]: resolveResult('successCooked'),
      },
      assetCapabilitySupported: true,
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    expect(screen.getByText('cooked OK')).toBeTruthy();
    expect(screen.getAllByText('未検証')).toHaveLength(1);
  });

  it('shows selected hash mismatch while non-selected rows remain 未検証', () => {
    const selectedKey = assetKeyForEntry(DEMO_MANIFEST.assets[0]!);
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
      selectedAssetKey: selectedKey,
      connection: { status: 'connected' },
      assetResolveByKey: {
        [selectedKey]: resolveResult('cookedEntryHashMismatch'),
      },
      assetCapabilitySupported: true,
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    expect(screen.getByText('hash mismatch')).toBeTruthy();
    expect(screen.getAllByText('未検証')).toHaveLength(1);
  });

  it('shows 確認中 for the selected connected row before a result arrives', () => {
    const selectedKey = assetKeyForEntry(DEMO_MANIFEST.assets[1]!);
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
      selectedAssetKey: selectedKey,
      connection: { status: 'connected' },
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    expect(screen.getByText('確認中')).toBeTruthy();
    expect(screen.getAllByText('未検証')).toHaveLength(1);
  });

  it('shows a selected asset resolve failure as 未確定 without a manifest banner', () => {
    const selectedKey = assetKeyForEntry(DEMO_MANIFEST.assets[0]!);
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
      selectedAssetKey: selectedKey,
      connection: { status: 'connected' },
      assetResolveErrorByKey: { [selectedKey]: { kind: 'request', message: 'timeout' } },
    };
    render(<AssetBrowserPanel {...makeDockviewProps()} />);
    expect(screen.getByText('未確定')).toBeTruthy();
    expect(screen.getAllByText('未検証')).toHaveLength(1);
    // A per-asset resolve failure must NOT raise the manifest-level error banner.
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('keeps rows stable across connect and disconnect while health changes', () => {
    mockState = {
      ...INITIAL_STATE,
      assetManifest: DEMO_MANIFEST,
      connection: { status: 'connected' },
    };
    const { rerender } = render(<AssetBrowserPanel {...makeDockviewProps()} />);
    expect(screen.getByText('textures/hero.png')).toBeTruthy();
    expect(screen.getByText('materials/hero.mat')).toBeTruthy();
    expect(screen.getAllByText('未検証')).toHaveLength(2);

    mockState = {
      ...mockState,
      connection: { status: 'disconnected' },
      assetResolveByKey: undefined,
      assetCapabilitySupported: undefined,
    };
    rerender(<AssetBrowserPanel {...makeDockviewProps()} />);
    expect(screen.getByText('textures/hero.png')).toBeTruthy();
    expect(screen.getByText('materials/hero.mat')).toBeTruthy();
    expect(screen.getAllByText('未検証(未接続)')).toHaveLength(2);
  });
});
