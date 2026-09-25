// Лидерборд: таблица рангов (admin-only на бэке)

import { h } from '../core/dom.js';
import { emptyState, skeleton, table } from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import { t } from '../core/i18n.js';

const MEDALS = ['🥇', '🥈', '🥉'];

function rankCell(rank) {
  if (rank >= 1 && rank <= 3) {
    return h('span.row', { style: { gap: '6px', justifyContent: 'flex-end' } },
      h('span', { text: MEDALS[rank - 1] }),
      h('span.num', { text: String(rank) })
    );
  }
  return h('span.num', { text: String(rank) });
}

export function renderPage(root, params = {}) {
  const marker = h('span', { style: { display: 'none' }, 'data-page': 'leaderboard' });
  const isCurrent = () => marker.isConnected;
  const host = h('div');

  root.replaceChildren(
    marker,
    h('div.page-header', null,
      h('div', null,
        h('h1', { text: t('leaderboard.title') }),
        h('div.page-sub', null, t('leaderboard.sub'))
      )
    ),
    host
  );

  host.append(skeleton(6, 32));

  request('/stats/leaderboard', { query: { limit: 50 } })
    .then((rows) => {
      if (!isCurrent()) return;
      if (!rows || !rows.length) {
        host.replaceChildren(emptyState({
          icon: '★',
          title: t('leaderboard.empty'),
          description: t('leaderboard.emptyDesc'),
        }));
        return;
      }
      host.replaceChildren(table({
        columns: [
          {
            key: 'rank',
            label: t('leaderboard.col.rank'),
            align: 'right',
            render: (r) => rankCell(r.rank),
          },
          {
            key: 'login',
            label: t('leaderboard.col.login'),
            render: (r) => r.display_name
              ? h('span', null,
                  h('strong', { text: r.display_name }),
                  h('span.muted', { text: ` (${r.login})` }))
              : r.login,
          },
          {
            key: 'finished_sessions',
            label: t('leaderboard.col.finished'),
            align: 'right',
            mono: true,
          },
          {
            key: 'xp',
            label: t('leaderboard.col.xp'),
            align: 'right',
            mono: true,
            render: (r) => h('strong.num', { text: `${r.xp}` }),
          },
          {
            key: 'level',
            label: t('leaderboard.col.level'),
            align: 'right',
            mono: true,
            render: (r) => h('span.num', { text: String(r.level) }),
          },
          {
            key: 'best_score',
            label: t('leaderboard.col.best'),
            align: 'right',
            mono: true,
            render: (r) => h('strong.num', { text: String(r.best_score) }),
          },
          {
            key: 'avg_score',
            label: t('leaderboard.col.avg'),
            align: 'right',
            mono: true,
          },
        ],
        rows,
        emptyText: t('common.noData'),
      }));
    })
    .catch((err) => {
      if (!isCurrent()) return;
      if (err instanceof ApiError && err.status === 403) {
        host.replaceChildren(emptyState({
          icon: '🔒',
          title: t('leaderboard.adminOnly'),
          description: t('leaderboard.adminOnlyDesc'),
        }));
        return;
      }
      host.replaceChildren(emptyState({
        icon: '⚠',
        title: t('leaderboard.loadError'),
        description: err instanceof ApiError ? err.message : t('common.error'),
        action: h('button.btn.btn-secondary', {
          type: 'button',
          text: t('action.retry'),
          onClick: () => { if (isCurrent()) renderPage(root, params); },
        }),
      }));
    });
}
