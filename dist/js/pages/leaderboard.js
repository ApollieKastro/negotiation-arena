// Лидерборд: таблица рангов (admin-only на бэке)

import { h } from '../core/dom.js';
import { emptyState, skeleton, table } from '../core/components.js';
import { request, ApiError } from '../core/api.js';

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
        h('h1', { text: 'Лидерборд' }),
        h('div.page-sub', null, 'Лучшие переговорщики по итоговому баллу')
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
          title: 'Пока пусто',
          description: 'Ещё нет завершённых сессий — таблица появится позже.',
        }));
        return;
      }
      host.replaceChildren(table({
        columns: [
          {
            key: 'rank',
            label: '#',
            align: 'right',
            render: (r) => rankCell(r.rank),
          },
          {
            key: 'login',
            label: 'Логин',
            render: (r) => r.display_name
              ? h('span', null,
                  h('strong', { text: r.display_name }),
                  h('span.muted', { text: ` (${r.login})` }))
              : r.login,
          },
          {
            key: 'finished_sessions',
            label: 'Завершено',
            align: 'right',
            mono: true,
          },
          {
            key: 'best_score',
            label: 'Лучший балл',
            align: 'right',
            mono: true,
            render: (r) => h('strong.num', { text: String(r.best_score) }),
          },
          {
            key: 'avg_score',
            label: 'Средний',
            align: 'right',
            mono: true,
          },
        ],
        rows,
        emptyText: 'Нет данных',
      }));
    })
    .catch((err) => {
      if (!isCurrent()) return;
      if (err instanceof ApiError && err.status === 403) {
        host.replaceChildren(emptyState({
          icon: '🔒',
          title: 'Доступно администраторам',
          description: 'Лидерборд показывает сводную статистику пользователей — раздел доступен только админам.',
        }));
        return;
      }
      host.replaceChildren(emptyState({
        icon: '⚠',
        title: 'Не удалось загрузить лидерборд',
        description: err instanceof ApiError ? err.message : 'Ошибка запроса',
        action: h('button.btn.btn-secondary', {
          type: 'button',
          text: 'Повторить',
          onClick: () => { if (isCurrent()) renderPage(root, params); },
        }),
      }));
    });
}
