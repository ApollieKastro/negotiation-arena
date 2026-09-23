// Админ-дашборд: обзор платформы, активность за 30 дней, лидерборд

import { h, fmtDate } from '../../core/dom.js';
import { request, ApiError } from '../../core/api.js';
import { statCard, table, skeleton, emptyState, toast } from '../../core/components.js';

function errMsg(err, fallback) {
  return err instanceof ApiError ? err.message : fallback;
}

/** Контракт страницы: рендер в root (DOM-контейнер) + params роутера */
export function renderPage(root, params = {}) {
  const actions = h('div.page-actions', null,
    h('a.btn.btn-primary.btn-sm', { href: '#/admin/scenarios' }, '+ Сценарий'),
    h('a.btn.btn-secondary.btn-sm', { href: '#/admin/users' }, 'Пользователи'),
    h('a.btn.btn-secondary.btn-sm', { href: '#/admin/providers' }, 'Провайдеры'),
    h('a.btn.btn-secondary.btn-sm', { href: '#/admin/settings' }, 'Настройки'),
    h('a.btn.btn-secondary.btn-sm', { href: '#/admin/audit' }, 'Аудит')
  );

  const overviewHost = h('div');
  const activityHost = h('div');
  const leadersHost = h('div');

  root.replaceChildren(
    h('div.page-header', null,
      h('div', null,
        h('h1', null, 'Админ-дашборд'),
        h('div.page-sub', null, 'Обзор платформы и активности')
      ),
      actions
    ),
    overviewHost,
    activityHost,
    leadersHost
  );

  loadOverview(overviewHost);
  loadActivity(activityHost);
  loadLeaderboard(leadersHost);
}

function loadOverview(host) {
  host.replaceChildren(h('div.stat-grid', null, skeleton(4, 72)));
  request('/stats/overview')
    .then((ov) => {
      const cards = h('div.stat-grid', null,
        statCard({ label: 'Пользователи', value: ov.users, hint: `активных: ${ov.active_users}` }),
        statCard({
          label: 'Сессии',
          value: ov.sessions,
          hint: `завершено ${ov.finished_sessions} · активных ${ov.active_sessions}`,
        }),
        statCard({ label: 'Сценарии', value: ov.scenarios, hint: `опубликовано ${ov.active_scenarios}` }),
        statCard({ label: 'Средний балл', value: ov.avg_finished_score, hint: 'по завершённым сессиям' }),
        statCard({ label: 'Лучший балл', value: ov.best_score, hint: 'максимум на платформе' })
      );
      host.replaceChildren(cards);
    })
    .catch((err) => {
      host.replaceChildren(
        h('div.card', null,
          h('div.card-body', null,
            h('div.text-danger', { text: errMsg(err, 'Не удалось загрузить обзор платформы') })
          )
        )
      );
    });
}

function loadActivity(host) {
  host.replaceChildren(
    h('div.card', { style: { marginBottom: 'var(--sp-4)' } },
      h('div.card-header', null, h('div.card-title', { text: 'Активность за 30 дней' })),
      h('div.card-body', null, skeleton(3))
    )
  );

  request('/stats/activity', { query: { days: 30 } })
    .then((points) => {
      const rawMax = Math.max(0, ...points.map((p) => p.sessions));
      const max = Math.max(1, rawMax);
      const bars = h('div', {
        style: {
          display: 'flex',
          alignItems: 'flex-end',
          gap: '2px',
          height: '160px',
          padding: '8px 0',
        },
      });
      for (const p of points) {
        const pct = Math.round((p.sessions / max) * 100);
        bars.append(
          h('div', {
            title: `${fmtDate(p.date)}: ${p.sessions}`,
            style: {
              flex: '1',
              minWidth: '4px',
              height: `${Math.max(p.sessions > 0 ? 4 : 2, pct)}%`,
              background: 'var(--accent)',
              borderRadius: '2px 2px 0 0',
              opacity: p.sessions > 0 ? '1' : '0.3',
            },
          })
        );
      }

      const caption = h('div.row-between.mt-3', null,
        h('span.small.muted', { text: points.length ? fmtDate(points[0].date) : '—' }),
        h('span.small.muted', { text: `макс. за день: ${rawMax}` }),
        h('span.small.muted', { text: points.length ? fmtDate(points[points.length - 1].date) : '—' })
      );

      host.replaceChildren(
        h('div.card', { style: { marginBottom: 'var(--sp-4)' } },
          h('div.card-header', null,
            h('div.card-title', { text: 'Активность за 30 дней' }),
            h('span.small.muted', { text: 'сессий по дням' })
          ),
          h('div.card-body', null,
            points.every((p) => p.sessions === 0)
              ? emptyState({ icon: '∅', title: 'Сессий пока нет', description: 'Гистограмма оживёт, как только появятся сессии.' })
              : bars,
            points.every((p) => p.sessions === 0) ? null : caption
          )
        )
      );
    })
    .catch((err) => {
      host.replaceChildren(
        h('div.card', null,
          h('div.card-body', null,
            h('div.text-danger', { text: errMsg(err, 'Не удалось загрузить активность') })
          )
        )
      );
    });
}

function loadLeaderboard(host) {
  host.replaceChildren(
    h('div.card', null,
      h('div.card-header', null, h('div.card-title', { text: 'Лидерборд — топ-10' })),
      h('div.card-body', null, skeleton(5))
    )
  );

  request('/stats/leaderboard', { query: { limit: 10 } })
    .then((rows) => {
      const body = rows.length
        ? table({
            columns: [
              { key: 'rank', label: '#', mono: true },
              { key: 'login', label: 'Логин' },
              {
                key: 'display_name',
                label: 'Имя',
                render: (r) => r.display_name || '',
              },
              { key: 'finished_sessions', label: 'Завершено', align: 'right', mono: true },
              { key: 'best_score', label: 'Лучший балл', align: 'right', mono: true },
              { key: 'avg_score', label: 'Средний', align: 'right', mono: true },
            ],
            rows,
            emptyText: 'Пока нет завершённых сессий',
          })
        : emptyState({
            icon: '★',
            title: 'Лидерборд пуст',
            description: 'Топ появится, когда пользователи начнут завершать сессии.',
          });

      host.replaceChildren(
        h('div.card', null,
          h('div.card-header', null,
            h('div.card-title', { text: 'Лидерборд — топ-10' }),
            h('a.small', { href: '#/leaderboard' }, 'Открыть полный →')
          ),
          h('div.card-body', null, body)
        )
      );
    })
    .catch((err) => {
      toast(errMsg(err, 'Не удалось загрузить лидерборд'), 'error');
      host.replaceChildren(
        h('div.card', null,
          h('div.card-body', null,
            h('div.text-danger', { text: errMsg(err, 'Не удалось загрузить лидерборд') })
          )
        )
      );
    });
}
