// Кабинет: приветствие, статистика, активные сценарии, последние сессии

import { h, fmtDateTime, plural } from '../core/dom.js';
import {
  emptyState, skeleton, statCard, toast, badge,
} from '../core/components.js';
import { request, ApiError } from '../core/api.js';
import { user } from '../core/store.js';
import { navigate } from '../core/router.js';

const DIFF_LABEL = { easy: 'Начальная', medium: 'Средняя', hard: 'Сложная' };
const DIFF_VARIANT = { easy: 'success', medium: 'warning', hard: 'danger' };
const STATUS_LABEL = { active: 'Активна', finished: 'Завершена', abandoned: 'Брошена' };
const STATUS_VARIANT = { active: 'info', finished: 'success', abandoned: 'neutral' };

function truncate(text, max = 140) {
  const s = String(text || '');
  return s.length > max ? `${s.slice(0, max).trimEnd()}…` : s;
}

function statusBadge(status) {
  return badge(STATUS_LABEL[status] || status, STATUS_VARIANT[status] || 'neutral');
}

async function startSession(scenarioId, btn) {
  if (btn) { btn.disabled = true; btn.textContent = 'Старт…'; }
  try {
    const started = await request('/sessions', {
      method: 'POST',
      body: { scenario_id: scenarioId },
    });
    navigate(`#/session/${started.session.id}`);
  } catch (err) {
    toast(err instanceof ApiError ? err.message : 'Не удалось начать сессию', 'error');
    if (btn) { btn.disabled = false; btn.textContent = 'Начать'; }
  }
}

function scenarioCard(s) {
  const startBtn = h('button.btn.btn-primary.btn-sm', {
    type: 'button',
    text: 'Начать',
    onClick: () => startSession(s.id, startBtn),
  });
  return h('div.card', null,
    h('div.card-body.stack', { style: { gap: 'var(--sp-3)' } },
      h('div.row-between', null,
        h('div.card-title', { text: s.title }),
        badge(DIFF_LABEL[s.difficulty] || s.difficulty, DIFF_VARIANT[s.difficulty] || 'neutral')
      ),
      h('div.small.muted', { text: s.sphere || '' }),
      h('div', { text: truncate(s.description, 140), style: { color: 'var(--muted)' } }),
      s.player_goal
        ? h('div.small', null,
            h('span.muted', { text: 'Цель: ' }),
            h('span', { text: truncate(s.player_goal, 100) }))
        : null,
      h('div.row', null, startBtn)
    )
  );
}

function sessionRow(s, titleMap) {
  const score = s.status === 'finished'
    ? h('strong.num', { text: String(s.total_score) })
    : h('span.muted', { text: '—' });
  return h('div.row-between.card', {
    style: { padding: 'var(--sp-3) var(--sp-4)', cursor: 'pointer' },
    onClick: () => {
      navigate(s.status === 'finished' ? `#/result/${s.id}` : `#/session/${s.id}`);
    },
  },
    h('div.stack', { style: { gap: '2px' } },
      h('div', { text: titleMap.get(s.scenario_id) || 'Сценарий' }),
      h('div.small.muted', { text: fmtDateTime(s.created_at) })
    ),
    h('div.row', null,
      h('span.small', null, `${s.turn_count} ${plural(s.turn_count, ['ход', 'хода', 'ходов'])}`),
      statusBadge(s.status),
      score
    )
  );
}

export function renderPage(root, params = {}) {
  const u = user() || {};
  const name = u.display_name || u.login || 'игрок';
  const marker = h('span', { style: { display: 'none' }, 'data-page': 'home' });
  const isCurrent = () => marker.isConnected;

  const statsHost = h('div.stat-grid');
  const scenariosHost = h('div.grid-2');
  const recentHost = h('div.stack');

  root.replaceChildren(
    marker,
    h('div.page-header', null,
      h('div', null,
        h('h1', null, `Привет, ${name}!`),
        h('div.page-sub', null, 'Кабинет: статистика, сценарии и последние сессии')
      ),
      h('div.page-actions', null,
        h('a.btn.btn-primary', { href: '#/scenarios' }, 'Все сценарии')
      )
    ),

    h('section.mt-4', null,
      h('div.row-between.mb-4', null, h('h2', { text: 'Моя статистика' })),
      statsHost
    ),

    h('section.mt-4', null,
      h('div.row-between.mb-4', null,
        h('h2', { text: 'Сценарии' }),
        h('a.small', { href: '#/scenarios' }, 'Смотреть все →')
      ),
      scenariosHost
    ),

    h('section.mt-4', null,
      h('div.row-between.mb-4', null, h('h2', { text: 'Последние сессии' })),
      recentHost
    )
  );

  // ── Статистика ──
  statsHost.append(skeleton(3, 48));
  request('/stats/me').then((st) => {
    if (!isCurrent()) return;
    statsHost.replaceChildren(
      statCard({ label: 'Всего сессий', value: st.total_sessions }),
      statCard({ label: 'Завершено', value: st.finished }),
      statCard({ label: 'Лучший балл', value: st.best_score }),
      statCard({ label: 'Средний балл', value: st.avg_score }),
      statCard({ label: 'Активных', value: st.active })
    );
  }).catch((err) => {
    if (!isCurrent()) return;
    statsHost.replaceChildren(
      h('div.card', null,
        h('div.card-body.small.text-danger', {
          text: err instanceof ApiError ? err.message : 'Не удалось загрузить статистику',
        })
      )
    );
  });

  // ── Сценарии (только активные) ──
  scenariosHost.append(skeleton(2, 120));
  request('/scenarios').then((list) => {
    if (!isCurrent()) return;
    const active = (list || []).filter((s) => s.is_active !== false).slice(0, 4);
    if (!active.length) {
      scenariosHost.replaceChildren(emptyState({
        icon: '◈',
        title: 'Сценариев пока нет',
        description: 'Администратор ещё не опубликовал сценарии.',
      }));
      return;
    }
    scenariosHost.replaceChildren(...active.map(scenarioCard));
  }).catch((err) => {
    if (!isCurrent()) return;
    scenariosHost.replaceChildren(emptyState({
      icon: '⚠',
      title: 'Не удалось загрузить сценарии',
      description: err instanceof ApiError ? err.message : 'Ошибка запроса',
    }));
  });

  // ── Последние сессии ──
  recentHost.append(skeleton(3, 40));
  Promise.all([
    request('/sessions', { query: { limit: 5 } }),
    request('/scenarios').catch(() => []),
  ]).then(([page, scenarios]) => {
    if (!isCurrent()) return;
    // Совместимость: `{items,...}` (новый формат) или массив (fallback).
    const sessions = Array.isArray(page)
      ? page
      : Array.isArray(page?.items) ? page.items : [];
    const titleMap = new Map((scenarios || []).map((s) => [s.id, s.title]));
    if (!sessions.length) {
      recentHost.replaceChildren(emptyState({
        icon: '☰',
        title: 'Сессий ещё нет',
        description: 'Начните первый сценарий — он появится здесь.',
        action: h('a.btn.btn-primary', { href: '#/scenarios' }, 'Выбрать сценарий'),
      }));
      return;
    }
    recentHost.replaceChildren(...sessions.map((s) => sessionRow(s, titleMap)));
  }).catch((err) => {
    if (!isCurrent()) return;
    recentHost.replaceChildren(emptyState({
      icon: '⚠',
      title: 'Не удалось загрузить историю',
      description: err instanceof ApiError ? err.message : 'Ошибка запроса',
    }));
  });
}
