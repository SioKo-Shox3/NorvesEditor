/** MCP書き込みの影響を確認し、main画面から一度だけ決定する。 */
import { useContext } from 'react';
import type React from 'react';
import type { IDockviewPanelProps } from 'dockview-react';
import { McpApprovalsContext } from '../hooks/useMcpApprovals.js';

function preview(value: unknown): string {
  return value === undefined ? '該当なし' : JSON.stringify(value, null, 2);
}

export function McpApprovalPanel(_props: IDockviewPanelProps): React.JSX.Element {
  const state = useContext(McpApprovalsContext);
  if (!state) return <div className="panel">確認はメイン画面で行ってください。</div>;
  const canApprove = state.mode === 'enabled' || state.mode === 'confirm';
  return (
    <section className="panel mcp-approvals" aria-label="MCPの書き込み確認">
      <div className="panel__header">MCP 確認待ち（{state.requests.length}件）</div>
      <div className="panel__body col">
        <p>確認期限は最大120秒です。待機中もエディタで編集できます。</p>
        {!canApprove && <p>読み取りのみ、または許可を確認中のため承認できません。</p>}
        {state.error && <p className="error-banner" role="alert">{state.error}</p>}
        {state.loading && <p>確認待ちを読み込み中…</p>}
        {!state.loading && state.requests.length === 0 && <p>確認待ちはありません。</p>}
        {state.requests.map((request, index) => (
          <article className="mcp-approvals__request" key={request.id} aria-label={`確認 ${index + 1}`}>
            <h3>{request.toolName}</h3>
            <dl className="mcp-approvals__details">
              <dt>操作</dt><dd>{request.method}</dd>
              <dt>要求元</dt><dd>MCP（言語モデル）</dd>
              <dt>対象</dt>
              <dd>
                {request.targetCount}件
                {request.targetIds.length > 0 && <pre>{request.targetIds.join('\n')}</pre>}
                {request.targetCount > request.targetIds.length && <p>対象IDは一部を表示しています。</p>}
              </dd>
              <dt>履歴の出どころ</dt>
              <dd>{request.source === 'ui' ? '人の編集' : request.source === 'mcp' ? 'MCP（言語モデル）' : '該当なし'}</dd>
              <dt>変更前</dt><dd><pre>{preview(request.before)}</pre></dd>
              <dt>変更後</dt><dd><pre>{preview(request.after)}</pre></dd>
              <dt>取り消し</dt><dd>{request.undoAvailable ? '履歴から取り消し可能' : '取り消せません'}</dd>
              <dt>確認期限</dt><dd>{new Date(request.expiresAt).toLocaleTimeString('ja-JP')}</dd>
            </dl>
            {request.clearsHistory && (
              <p className="mcp-approvals__warning" role="note">
                削除すると、人の編集を含む取り消し・やり直しの全履歴が破棄されます。
              </p>
            )}
            <div className="row">
              {canApprove && (
                <button className="btn btn--primary" type="button"
                  disabled={state.busyIds.has(request.id)}
                  onClick={() => state.decide(request.id, true)}>
                  今回だけ承認
                </button>
              )}
              <button className="btn" type="button"
                disabled={state.busyIds.has(request.id)}
                onClick={() => state.decide(request.id, false)}>
                拒否
              </button>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}
