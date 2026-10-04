/** 編集まとまりの部分失敗と復旧操作を画面へ表示する。 */

import { useEffect, useRef, useState } from 'react';
import type React from 'react';
import { createPortal } from 'react-dom';
import { useBridgeActions } from '../hooks/useBridge.js';
import { useBridgeState } from '../state/BridgeContext.js';

function sourceLabel(source: 'ui' | 'mcp' | undefined): string {
  if (source === 'mcp') return 'MCP';
  if (source === 'ui') return '画面操作';
  return '不明';
}

export function HistoryProblemNotice(): React.JSX.Element | null {
  const state = useBridgeState();
  const actions = useBridgeActions();
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const firstActionRef = useRef<HTMLButtonElement>(null);
  const history = state.editHistorySummary;
  const pending = history?.pending === true;
  const pendingGroup = pending ? history?.pendingGroup ?? undefined : undefined;
  const result = pending ? undefined : state.editDiscardResult;

  useEffect(() => {
    if (pending) {
      firstActionRef.current?.focus();
    }
  }, [pending]);

  useEffect(() => {
    if (!pending) return;
    const editorBody = document.querySelector<HTMLElement>('.app-shell__body');
    if (editorBody === null) return;
    const wasInert = editorBody.inert;
    editorBody.inert = true;
    return () => {
      editorBody.inert = wasInert;
    };
  }, [pending]);

  useEffect(() => {
    if (!pending) return;

    const isShortcut = (event: KeyboardEvent): boolean =>
      (event.ctrlKey || event.metaKey) &&
      (event.key.toLowerCase() === 'z' || event.key.toLowerCase() === 'y');

    const onKeyDown = (event: KeyboardEvent): void => {
      const target = event.target instanceof Element ? event.target : null;
      const insideNotice = target !== null && target.closest('[data-history-problem-notice]') !== null;
      const insideToolbar = target !== null && target.closest('.toolbar, .titlebar') !== null;

      if (isShortcut(event)) {
        event.preventDefault();
        event.stopImmediatePropagation();
        return;
      }

      if (insideNotice) {
        if (event.key === 'Tab') {
          const buttons = Array.from(
            document.querySelectorAll<HTMLButtonElement>(
              '[data-history-problem-notice] button:not(:disabled)',
            ),
          );
          const first = buttons[0];
          const last = buttons.at(-1);
          if (first !== undefined && last !== undefined) {
            if (event.shiftKey && document.activeElement === first) {
              event.preventDefault();
              last.focus();
            } else if (!event.shiftKey && document.activeElement === last) {
              event.preventDefault();
              first.focus();
            }
          }
        }
        return;
      }

      if (insideToolbar) {
        if (event.key === 'Tab') {
          event.preventDefault();
          const buttons = Array.from(
            document.querySelectorAll<HTMLButtonElement>(
              '[data-history-problem-notice] button:not(:disabled)',
            ),
          );
          (event.shiftKey ? buttons.at(-1) : buttons[0])?.focus();
        }
        return;
      }

      if (event.key === 'Tab') {
        event.preventDefault();
        const buttons = Array.from(
          document.querySelectorAll<HTMLButtonElement>(
            '[data-history-problem-notice] button:not(:disabled)',
          ),
        );
        (event.shiftKey ? buttons.at(-1) : buttons[0])?.focus();
      }
    };

    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [pending]);

  const runAction = async (action: () => Promise<void>): Promise<void> => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    try {
      await action();
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  };

  if (pending) {
    const groupSummary = pendingGroup?.direction === 'redo'
      ? history?.redoGroup
      : history?.undoGroup;
    const groupName = pendingGroup?.name ?? groupSummary?.name ?? '編集まとまり';
    const source = pendingGroup?.source ?? groupSummary?.source;
    const total = pendingGroup?.totalCount ?? groupSummary?.count;
    const completed = pendingGroup?.completedCount ?? 0;
    const outcomeUnknown = pendingGroup?.outcomeUnknown === true;
    const canRetry =
      pendingGroup !== undefined &&
      pendingGroup.retryAllowed &&
      !outcomeUnknown;

    return createPortal((
      <>
        <div className="history-problem-shield" aria-hidden="true" />
        <section
          className="history-problem-notice"
          role="alertdialog"
          aria-labelledby="history-problem-title"
          aria-describedby="history-problem-description"
          data-history-problem-notice
        >
          <h2 id="history-problem-title">編集の一部に失敗しました</h2>
          <p className="history-problem-notice__summary">
            <strong>{groupName}</strong>
            <span>出どころ: {sourceLabel(source)}</span>
            {total !== undefined && (
              <span>
                {completed} / {total} 件を処理済み
              </span>
            )}
            {pendingGroup !== undefined && (
              <span>方向: {pendingGroup.direction === 'undo' ? '取り消し' : 'やり直し'}</span>
            )}
          </p>
          <p id="history-problem-description">
            {outcomeUnknown
              ? '応答が途切れた操作の結果は不明です。重複を避けるため再試行できません。状態を確認するか、まとまりを破棄してください。'
              : canRetry
                ? '保留中は通常の編集、取り消し、やり直し、実行制御を停止しています。再試行すると未処理の操作だけを続けます。'
                : '保留中は通常の編集、取り消し、やり直し、実行制御を停止しています。まず状態を確認してください。'}
          </p>
          <p className="history-problem-notice__warning">
            破棄しても処理済みの変更は元に戻りません。一部の変更が残る場合があります。
          </p>
          <div className="history-problem-notice__actions">
            {canRetry && (
              <button
                ref={firstActionRef}
                className="btn btn--primary"
                type="button"
                disabled={busy}
                onClick={() =>
                  void runAction(() => actions.retryPendingEdit?.() ?? Promise.resolve())
                }
              >
                再試行
              </button>
            )}
            <button
              ref={canRetry ? undefined : firstActionRef}
              className="btn"
              type="button"
              disabled={busy}
              onClick={() =>
                void runAction(() => actions.refreshEditHistory?.() ?? Promise.resolve())
              }
            >
              状態を確認
            </button>
            <button
              className="btn btn--danger"
              type="button"
              disabled={busy}
              onClick={() =>
                void runAction(() => actions.discardPendingEdit?.() ?? Promise.resolve())
              }
            >
              破棄
            </button>
          </div>
          {busy && <p role="status">処理しています…</p>}
        </section>
      </>
    ), document.body);
  }

  if (result === undefined) return null;

  return createPortal((
    <section className="history-problem-result" role="status" aria-live="polite">
      <div>
        <strong>保留中のまとまりを破棄しました</strong>
        <p>
          {result.outcomeUnknown
            ? `結果不明の操作が含まれます。処理済み ${result.completedCount} / ${result.totalCount} 件のほかにも変更が残っている可能性があります。`
            : result.changesRemain
              ? `処理済み ${result.completedCount} / ${result.totalCount} 件の変更は元に戻らず、画面に残っています。`
              : '処理済みの変更はありません。'}
        </p>
      </div>
      <button
        className="btn"
        type="button"
        onClick={() => actions.dismissEditDiscardResult?.()}
      >
        閉じる
      </button>
    </section>
  ), document.body);
}
