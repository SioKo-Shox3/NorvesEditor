/** AIの操作記録、確認待ち、共通履歴へのリンクを表示する。 */
import type React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import type { McpOperation } from '@norves/bridge-ui';
import { useMcpOperations } from '../hooks/useMcpOperations.js';
import { McpApprovalPanel } from './McpApprovalPanel.js';

const RESULT_LABELS: Record<McpOperation['result'], string> = {
  success: '成功', rejected: '拒否', failed: '失敗', partial: '部分成功',
  timedOut: '時間切れ', cancelled: '取消', unknown: '結果不明',
};
const OUTCOME_LABELS: Record<McpOperation['outcome'], string> = {
  applied: '適用済み', noChange: '変更なし', notApplied: '未送信・未適用',
  rejected: '適用拒否', partial: '一部適用', unknown: '適用結果不明',
  readCompleted: '読み取り完了', readFailed: '読み取り失敗',
};

export function McpOperationsPanel(props: IDockviewPanelProps): React.JSX.Element {
  const state = useMcpOperations();
  return (
    <section className="panel mcp-operations" aria-label="AIの操作">
      <div className="panel__header">AI の操作</div>
      <div className="panel__body col">
        <McpApprovalPanel {...props} />
        <p>この起動の最新500件を表示します。対象は同じ起動内で照合できる指紋です。</p>
        <p>要求IDで道具の応答と照合できます。記録ファイルはログフォルダーの mcp-operations.jsonl です。過去の起動分も含め、現行と前世代の2ファイルに各1 MiBまで保存します。</p>
        <p>取り消しは人の編集を含む履歴の順番に従います。現在の先頭まとまりだけ取り消せます。</p>
        {state.error && <p className="error-banner" role="alert">{state.error}</p>}
        {state.undoError && <p className="error-banner" role="alert">{state.undoError}</p>}
        {state.snapshot?.storageError && (
          <p className="error-banner" role="alert">操作記録のファイル保存に失敗しています。{state.snapshot.storageError}</p>
        )}
        {state.loading && <p>操作記録を読み込み中…</p>}
        {!state.loading && state.snapshot?.records.length === 0 && <p>操作記録はありません。</p>}
        <ol className="mcp-operations__list" aria-label="操作記録">
          {[...(state.snapshot?.records ?? [])].reverse().map((record) => {
            const reason = state.undoUnavailableReason(record);
            return (
              <li key={record.requestId} className="mcp-operations__record">
                <dl className="mcp-approvals__details">
                  <dt>時刻</dt><dd><time dateTime={new Date(record.timestamp).toISOString()}>{new Date(record.timestamp).toLocaleString('ja-JP')}</time></dd>
                  <dt>道具</dt><dd>{record.tool}</dd>
                  <dt>対象</dt><dd>{record.target}</dd>
                  <dt>要約</dt><dd>{record.summary}</dd>
                  <dt>結果</dt><dd>{RESULT_LABELS[record.result]} / {OUTCOME_LABELS[record.outcome]}</dd>
                  <dt>確認済み件数</dt><dd>{record.completedCount}件</dd>
                  <dt>要求ID</dt><dd>{record.requestId}</dd>
                  <dt>まとまりID</dt><dd>{record.displayGroupId ?? 'なし'}</dd>
                </dl>
                {!record.actorFinished && <p>操作の結果を確認中です。自動再送は行いません。</p>}
                {record.outcome === 'unknown' && <p>適用結果が不明です。エンジンの状態を確認してください。自動再送は行いません。</p>}
                {record.pending && <p>
                  要求終了時に履歴の処理が保留になりました。
                  {record.retryAllowed ? '未処理部分は再試行可能です。' : '結果を確認するまで再試行できません。'}
                  現在の保留状態と復旧操作は画面上部で確認してください。自動再送は行いません。
                </p>}
                <button className="btn mcp-operations__undo" type="button"
                  disabled={reason !== undefined} title={reason}
                  aria-describedby={reason ? `undo-reason-${record.requestId}` : undefined}
                  onClick={() => { void state.undo(record.requestId); }}>
                  まとまりを取り消す
                </button>
                {reason && <p id={`undo-reason-${record.requestId}`}>{reason}</p>}
              </li>
            );
          })}
        </ol>
      </div>
    </section>
  );
}
